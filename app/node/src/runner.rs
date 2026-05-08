use crate::client::ClaimedDeployment;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub deployment_id: Uuid,
    pub repository_url: String,
    pub source_branch: String,
    pub workspace_dir: PathBuf,
    pub repo_dir: PathBuf,
    pub project_root: PathBuf,
    pub install_command: String,
    pub build_command: String,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub log: String,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DeploymentRunner {
    work_root: PathBuf,
}

#[derive(Debug)]
pub enum RunnerError {
    Validation(String),
    Io(std::io::Error),
    Command {
        stage: &'static str,
        command: String,
        exit_code: Option<i32>,
        log: String,
    },
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(message) => f.write_str(message),
            Self::Io(error) => write!(f, "{error}"),
            Self::Command {
                stage,
                command,
                exit_code,
                ..
            } => match exit_code {
                Some(code) => write!(f, "{stage} command `{command}` exited with status {code}"),
                None => write!(f, "{stage} command `{command}` terminated by signal"),
            },
        }
    }
}

impl std::error::Error for RunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Validation(_) | Self::Command { .. } => None,
        }
    }
}

impl DeploymentRunner {
    pub fn new(work_root: PathBuf) -> Self {
        Self { work_root }
    }

    pub fn build_plan(&self, deployment: &ClaimedDeployment) -> Result<DeploymentPlan, RunnerError> {
        if deployment.repository_url.trim().is_empty() {
            return Err(RunnerError::Validation(
                "repository_url is required".to_owned(),
            ));
        }
        if deployment.source_branch.trim().is_empty() {
            return Err(RunnerError::Validation(
                "source_branch is required".to_owned(),
            ));
        }
        if deployment.output_directory.trim().is_empty() {
            return Err(RunnerError::Validation(
                "output_directory is required".to_owned(),
            ));
        }

        let workspace_dir = self.work_root.join(deployment.id.to_string());
        let repo_dir = workspace_dir.join("repo");
        let project_root = match deployment.root_directory.as_deref().map(str::trim) {
            Some("") | None => repo_dir.clone(),
            Some(root_directory) => repo_dir.join(root_directory),
        };

        let output_dir = project_root.join(deployment.output_directory.trim());

        Ok(DeploymentPlan {
            deployment_id: deployment.id,
            repository_url: deployment.repository_url.clone(),
            source_branch: deployment.source_branch.clone(),
            workspace_dir,
            repo_dir,
            project_root,
            install_command: deployment.install_command.trim().to_owned(),
            build_command: deployment.build_command.trim().to_owned(),
            output_dir,
        })
    }

    pub async fn execute(&self, deployment: &ClaimedDeployment) -> Result<ExecutionOutput, RunnerError> {
        let plan = self.build_plan(deployment)?;

        if plan.workspace_dir.exists() {
            std::fs::remove_dir_all(&plan.workspace_dir).map_err(RunnerError::Io)?;
        }
        std::fs::create_dir_all(&plan.workspace_dir).map_err(RunnerError::Io)?;

        let mut log = String::new();
        self.run_stage(
            "clone",
            "git clone",
            Command::new("git")
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg("--branch")
                .arg(&plan.source_branch)
                .arg(&plan.repository_url)
                .arg(&plan.repo_dir),
            &mut log,
        )
        .await?;

        if !plan.project_root.is_dir() {
            return Err(RunnerError::Validation(format!(
                "project root does not exist: {}",
                plan.project_root.display()
            )));
        }

        self.run_shell_stage("install", &plan.install_command, &plan.project_root, &mut log)
            .await?;
        self.run_shell_stage("build", &plan.build_command, &plan.project_root, &mut log)
            .await?;

        if !plan.output_dir.is_dir() {
            return Err(RunnerError::Validation(format!(
                "build output directory does not exist: {}",
                plan.output_dir.display()
            )));
        }

        Ok(ExecutionOutput {
            log,
            output_dir: plan.output_dir,
        })
    }

    async fn run_shell_stage(
        &self,
        stage: &'static str,
        command: &str,
        current_dir: &Path,
        log: &mut String,
    ) -> Result<(), RunnerError> {
        let mut shell = Command::new("sh");
        shell.arg("-lc").arg(command).current_dir(current_dir);

        self.run_stage(stage, command, &mut shell, log).await
    }

    async fn run_stage(
        &self,
        stage: &'static str,
        command_label: &str,
        command: &mut Command,
        log: &mut String,
    ) -> Result<(), RunnerError> {
        log.push_str(&format!("$ {command_label}\n"));
        let output = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(RunnerError::Io)?;

        if !output.stdout.is_empty() {
            log.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            log.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        if !log.ends_with('\n') {
            log.push('\n');
        }

        if output.status.success() {
            Ok(())
        } else {
            Err(RunnerError::Command {
                stage,
                command: command_label.to_owned(),
                exit_code: output.status.code(),
                log: log.clone(),
            })
        }
    }
}
