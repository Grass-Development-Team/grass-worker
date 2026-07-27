use std::io;
use std::path::Path;

use axum::{
    body::Body,
    http::{HeaderValue, Method, StatusCode, header},
    response::Response,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio_util::io::ReaderStream;

fn parse_range(value: &str, length: u64) -> Result<(u64, u64), ()> {
    if length == 0 {
        return Err(());
    }
    let range = value.strip_prefix("bytes=").ok_or(())?;
    if range.is_empty() || range.contains(',') {
        return Err(());
    }
    let (start, end) = range.split_once('-').ok_or(())?;
    match (start.is_empty(), end.is_empty()) {
        (true, true) => Err(()),
        (true, false) => {
            let suffix = end.parse::<u64>().map_err(|_| ())?;
            if suffix == 0 {
                return Err(());
            }
            let selected = suffix.min(length);
            Ok((length - selected, length - 1))
        }
        (false, true) => {
            let start = start.parse::<u64>().map_err(|_| ())?;
            if start >= length {
                return Err(());
            }
            Ok((start, length - 1))
        }
        (false, false) => {
            let start = start.parse::<u64>().map_err(|_| ())?;
            let end = end.parse::<u64>().map_err(|_| ())?;
            if start > end || start >= length {
                return Err(());
            }
            Ok((start, end.min(length - 1)))
        }
    }
}

fn build_empty(status: StatusCode) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

pub(super) async fn serve_file(
    path: &Path,
    method: &Method,
    range: Option<&HeaderValue>,
    status: StatusCode,
) -> io::Result<Response> {
    if method != Method::GET && method != Method::HEAD {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Body::empty())
            .unwrap_or_else(|_| build_empty(StatusCode::INTERNAL_SERVER_ERROR)));
    }

    let mut file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if mime == mime_guess::mime::TEXT_HTML {
        "public, max-age=0, must-revalidate"
    } else {
        "public, max-age=3600"
    };

    let selected_range = if status == StatusCode::OK {
        range.map(|value| {
            value
                .to_str()
                .map_err(|_| ())
                .and_then(|value| parse_range(value, length))
        })
    } else {
        None
    };
    let (response_status, start, end) = match selected_range {
        Some(Ok((start, end))) => (StatusCode::PARTIAL_CONTENT, start, end),
        Some(Err(())) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .body(Body::empty())
                .unwrap_or_else(|_| build_empty(StatusCode::INTERNAL_SERVER_ERROR)));
        }
        None => (status, 0, length.saturating_sub(1)),
    };
    let content_length = if length == 0 { 0 } else { end - start + 1 };

    let mut response = Response::builder()
        .status(response_status)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, content_length.to_string());
    if response_status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{length}"),
        );
    }

    if method == Method::HEAD {
        return Ok(response
            .body(Body::empty())
            .unwrap_or_else(|_| build_empty(StatusCode::INTERNAL_SERVER_ERROR)));
    }
    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }
    let stream = ReaderStream::new(file.take(content_length));
    Ok(response
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| build_empty(StatusCode::INTERNAL_SERVER_ERROR)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::{
        body::to_bytes,
        http::{Method, StatusCode, header},
    };

    use super::*;

    fn test_file() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "grass-static-range-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::write(&path, b"0123456789").unwrap();
        path
    }

    #[test]
    fn parses_bounded_open_and_suffix_ranges() {
        assert_eq!(parse_range("bytes=2-5", 10).unwrap(), (2, 5));
        assert_eq!(parse_range("bytes=7-", 10).unwrap(), (7, 9));
        assert_eq!(parse_range("bytes=-3", 10).unwrap(), (7, 9));
        assert!(parse_range("bytes=0-1,4-5", 10).is_err());
        assert!(parse_range("bytes=20-30", 10).is_err());
    }

    #[tokio::test]
    async fn head_and_range_return_correct_headers_and_bodies() {
        let path = test_file();

        let head = serve_file(&path, &Method::HEAD, None, StatusCode::OK)
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
        assert_eq!(head.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(to_bytes(head.into_body(), 16).await.unwrap().as_ref(), b"");

        let range = "bytes=2-5".parse().unwrap();
        let partial = serve_file(&path, &Method::GET, Some(&range), StatusCode::OK)
            .await
            .unwrap();
        assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(partial.headers()[header::CONTENT_LENGTH], "4");
        assert_eq!(partial.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(
            to_bytes(partial.into_body(), 16).await.unwrap().as_ref(),
            b"2345"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn invalid_ranges_and_methods_return_protocol_errors() {
        let path = test_file();

        let invalid = "bytes=20-30".parse().unwrap();
        let unsatisfied = serve_file(&path, &Method::GET, Some(&invalid), StatusCode::OK)
            .await
            .unwrap();
        assert_eq!(unsatisfied.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(unsatisfied.headers()[header::CONTENT_RANGE], "bytes */10");

        let method = serve_file(&path, &Method::POST, None, StatusCode::OK)
            .await
            .unwrap();
        assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(method.headers()[header::ALLOW], "GET, HEAD");
        assert_eq!(
            to_bytes(method.into_body(), 16).await.unwrap().as_ref(),
            b""
        );

        std::fs::remove_file(path).unwrap();
    }
}
