use std::collections::HashSet;

use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    TeamMemberRole, UserStatus, announcement, project, team_member, user, user_notification,
};

pub struct CreateProjectNotification<'a> {
    pub project: &'a project::Model,
    pub actor_user_id: Uuid,
    pub action: &'a str,
    pub reason: Option<String>,
    pub target_url: String,
}

pub struct CreateAnnouncementNotification {
    pub announcement_id: Uuid,
    pub actor_user_id: Uuid,
    pub title: String,
    pub content: String,
}

pub struct NotificationPage {
    pub notifications: Vec<user_notification::Model>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

pub fn notification_title(action: &str) -> &'static str {
    match action {
        "project.slug_updated" => "Project slug changed",
        "deployment.withdrawn" => "Deployment withdrawn",
        "deployment.republish_review_requested" => "Deployment submitted for review",
        "deployment.republish_queued" | "deployment.republished" => "Deployment republished",
        "domain.approved" => "Domain approved",
        "domain.rejected" => "Domain rejected",
        "domain.deleted" => "Domain deleted",
        "project.archived" => "Project archived",
        "project.unarchived" => "Project unarchived",
        "project.deleted" => "Project deleted",
        "project.restored" => "Project restored",
        "site.announcement" => "Announcement",
        _ => "Project updated",
    }
}

pub fn resolve_recipient_ids(
    members: &[team_member::Model],
    eligible_user_ids: &HashSet<Uuid>,
    creator_user_id: Option<Uuid>,
) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    let mut recipients = Vec::new();

    for member in members.iter().filter(|member| {
        member.deleted_at.is_none()
            && eligible_user_ids.contains(&member.user_id)
            && matches!(member.role, TeamMemberRole::Owner | TeamMemberRole::Admin)
    }) {
        if seen.insert(member.user_id) {
            recipients.push(member.user_id);
        }
    }

    if let Some(creator_user_id) = creator_user_id
        && members.iter().any(|member| {
            member.deleted_at.is_none()
                && member.user_id == creator_user_id
                && eligible_user_ids.contains(&member.user_id)
        })
        && seen.insert(creator_user_id)
    {
        recipients.push(creator_user_id);
    }

    recipients
}

pub async fn create_project_notification<C: ConnectionTrait>(
    db: &C,
    params: CreateProjectNotification<'_>,
) -> anyhow::Result<usize> {
    let members = team_member::Entity::find()
        .filter(team_member::Column::TeamId.eq(params.project.team_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    let member_user_ids = members
        .iter()
        .map(|member| member.user_id)
        .collect::<Vec<_>>();
    let eligible_user_ids = user::Entity::find()
        .filter(user::Column::Id.is_in(member_user_ids))
        .filter(user::Column::Status.eq(UserStatus::Active))
        .filter(user::Column::DeletedAt.is_null())
        .all(db)
        .await?
        .into_iter()
        .map(|user| user.id)
        .collect::<HashSet<_>>();
    let recipients = resolve_recipient_ids(
        &members,
        &eligible_user_ids,
        params.project.created_by_user_id,
    );
    if recipients.is_empty() {
        return Ok(0);
    }

    let actor = user::Entity::find_by_id(params.actor_user_id)
        .one(db)
        .await?;
    let actor_label = actor
        .as_ref()
        .and_then(|actor| actor.display_name.as_deref())
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .or_else(|| actor.map(|actor| actor.email))
        .unwrap_or_else(|| "Platform administrator".to_owned());
    let now = OffsetDateTime::now_utc();
    let count = recipients.len();

    user_notification::Entity::insert_many(recipients.into_iter().map(|recipient_user_id| {
        user_notification::ActiveModel {
            id: Set(Uuid::now_v7()),
            recipient_user_id: Set(recipient_user_id),
            actor_user_id: Set(Some(params.actor_user_id)),
            team_id: Set(Some(params.project.team_id)),
            project_id: Set(Some(params.project.id)),
            announcement_id: Set(None),
            action: Set(params.action.to_owned()),
            project_name: Set(Some(params.project.name.clone())),
            project_slug: Set(Some(params.project.slug.clone())),
            actor_label: Set(actor_label.clone()),
            title: Set(None),
            content: Set(None),
            reason: Set(params.reason.clone()),
            target_url: Set(params.target_url.clone()),
            read_at: Set(None),
            created_at: Set(now),
        }
    }))
    .exec_without_returning(db)
    .await?;

    Ok(count)
}

pub async fn create_announcement_notifications<C: ConnectionTrait>(
    db: &C,
    params: CreateAnnouncementNotification,
) -> anyhow::Result<usize> {
    let recipients = user::Entity::find()
        .filter(user::Column::Status.eq(UserStatus::Active))
        .filter(user::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    if recipients.is_empty() {
        return Ok(0);
    }

    let actor_label = user::Entity::find_by_id(params.actor_user_id)
        .one(db)
        .await?
        .map(|actor| {
            actor
                .display_name
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(actor.email)
        })
        .unwrap_or_else(|| "Platform administrator".to_owned());
    let count = recipients.len();
    let now = OffsetDateTime::now_utc();

    user_notification::Entity::insert_many(recipients.into_iter().map(|recipient| {
        user_notification::ActiveModel {
            id: Set(Uuid::now_v7()),
            recipient_user_id: Set(recipient.id),
            actor_user_id: Set(Some(params.actor_user_id)),
            team_id: Set(None),
            project_id: Set(None),
            announcement_id: Set(Some(params.announcement_id)),
            action: Set("site.announcement".to_owned()),
            project_name: Set(None),
            project_slug: Set(None),
            actor_label: Set(actor_label.clone()),
            title: Set(Some(params.title.clone())),
            content: Set(Some(params.content.clone())),
            reason: Set(None),
            target_url: Set("/notifications".to_owned()),
            read_at: Set(None),
            created_at: Set(now),
        }
    }))
    .exec_without_returning(db)
    .await?;

    Ok(count)
}

pub async fn list_for_user<C: ConnectionTrait>(
    db: &C,
    recipient_user_id: Uuid,
    page: u64,
    per_page: u64,
) -> anyhow::Result<NotificationPage> {
    let page = page.max(1);
    let per_page = match per_page {
        0 => 25,
        value => value.clamp(1, 100),
    };
    let query = user_notification::Entity::find()
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id));
    let total = query.clone().count(db).await?;
    let notifications = query
        .order_by_desc(user_notification::Column::CreatedAt)
        .order_by_desc(user_notification::Column::Id)
        .offset((page - 1).saturating_mul(per_page))
        .limit(per_page)
        .all(db)
        .await?;

    Ok(NotificationPage {
        notifications,
        page,
        per_page,
        total,
        total_pages: total.div_ceil(per_page),
    })
}

pub async fn unread_count<C: ConnectionTrait>(
    db: &C,
    recipient_user_id: Uuid,
) -> anyhow::Result<u64> {
    user_notification::Entity::find()
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id))
        .filter(user_notification::Column::ReadAt.is_null())
        .count(db)
        .await
        .map_err(Into::into)
}

pub async fn latest_auto_popup<C: ConnectionTrait>(
    db: &C,
    recipient_user_id: Uuid,
) -> anyhow::Result<Option<user_notification::Model>> {
    Ok(user_notification::Entity::find()
        .find_also_related(announcement::Entity)
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id))
        .filter(user_notification::Column::Action.eq("site.announcement"))
        .filter(user_notification::Column::ReadAt.is_null())
        .filter(announcement::Column::AutoPopup.eq(true))
        .order_by_desc(user_notification::Column::CreatedAt)
        .order_by_desc(user_notification::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .find_map(|(notification, announcement)| announcement.map(|_| notification)))
}

pub async fn mark_read<C: ConnectionTrait>(
    db: &C,
    recipient_user_id: Uuid,
    notification_id: Uuid,
) -> anyhow::Result<bool> {
    let updated = user_notification::Entity::update_many()
        .col_expr(
            user_notification::Column::ReadAt,
            Expr::value(Some(OffsetDateTime::now_utc())),
        )
        .filter(user_notification::Column::Id.eq(notification_id))
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id))
        .filter(user_notification::Column::ReadAt.is_null())
        .exec(db)
        .await?;
    if updated.rows_affected > 0 {
        return Ok(true);
    }

    user_notification::Entity::find()
        .filter(user_notification::Column::Id.eq(notification_id))
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id))
        .one(db)
        .await
        .map(|item| item.is_some())
        .map_err(Into::into)
}

pub async fn mark_all_read<C: ConnectionTrait>(
    db: &C,
    recipient_user_id: Uuid,
) -> anyhow::Result<u64> {
    user_notification::Entity::update_many()
        .col_expr(
            user_notification::Column::ReadAt,
            Expr::value(Some(OffsetDateTime::now_utc())),
        )
        .filter(user_notification::Column::RecipientUserId.eq(recipient_user_id))
        .filter(user_notification::Column::ReadAt.is_null())
        .exec(db)
        .await
        .map(|result| result.rows_affected)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::infra::database::entity::{
        PlatformRole, ProjectRuntime, TeamMemberRole, UserStatus, project, team_member, user,
    };

    use super::{mark_read, notification_title, resolve_recipient_ids};

    fn member(user_id: Uuid, role: TeamMemberRole, deleted: bool) -> team_member::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        team_member::Model {
            id: Uuid::now_v7(),
            team_id: Uuid::nil(),
            user_id,
            role,
            invited_by_user_id: None,
            joined_at: now,
            deleted_at: deleted.then_some(now),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn recipients_are_active_owners_admins_and_the_active_creator_without_duplicates() {
        let owner = Uuid::now_v7();
        let admin = Uuid::now_v7();
        let creator = Uuid::now_v7();
        let viewer = Uuid::now_v7();
        let deleted_admin = Uuid::now_v7();
        let members = vec![
            member(owner, TeamMemberRole::Owner, false),
            member(admin, TeamMemberRole::Admin, false),
            member(creator, TeamMemberRole::Member, false),
            member(viewer, TeamMemberRole::Viewer, false),
            member(deleted_admin, TeamMemberRole::Admin, true),
        ];

        let recipients = resolve_recipient_ids(
            &members,
            &HashSet::from([owner, admin, creator, viewer]),
            Some(creator),
        );

        assert_eq!(recipients, vec![owner, admin, creator]);
    }

    #[test]
    fn creator_must_still_belong_to_the_team_and_is_deduplicated_when_already_admin() {
        let admin_creator = Uuid::now_v7();
        let former_creator = Uuid::now_v7();
        let members = vec![member(admin_creator, TeamMemberRole::Admin, false)];

        assert_eq!(
            resolve_recipient_ids(
                &members,
                &HashSet::from([admin_creator]),
                Some(admin_creator),
            ),
            vec![admin_creator]
        );
        assert_eq!(
            resolve_recipient_ids(
                &members,
                &HashSet::from([admin_creator, former_creator]),
                Some(former_creator),
            ),
            vec![admin_creator]
        );
    }

    #[test]
    fn disabled_or_deleted_users_are_not_recipients_even_with_active_memberships() {
        let disabled_owner = Uuid::now_v7();
        let active_admin = Uuid::now_v7();
        let members = vec![
            member(disabled_owner, TeamMemberRole::Owner, false),
            member(active_admin, TeamMemberRole::Admin, false),
        ];

        assert_eq!(
            resolve_recipient_ids(&members, &HashSet::from([active_admin]), None),
            vec![active_admin]
        );
    }

    #[test]
    fn governance_actions_have_stable_user_facing_titles() {
        assert_eq!(
            notification_title("project.slug_updated"),
            "Project slug changed"
        );
        assert_eq!(
            notification_title("deployment.withdrawn"),
            "Deployment withdrawn"
        );
        assert_eq!(
            notification_title("deployment.republished"),
            "Deployment republished"
        );
        assert_eq!(notification_title("domain.approved"), "Domain approved");
        assert_eq!(notification_title("domain.rejected"), "Domain rejected");
        assert_eq!(notification_title("domain.deleted"), "Domain deleted");
        assert_eq!(notification_title("project.archived"), "Project archived");
        assert_eq!(notification_title("project.restored"), "Project restored");
    }

    #[test]
    fn announcements_use_a_stable_action_for_content_notifications() {
        assert_eq!(notification_title("site.announcement"), "Announcement");
    }

    #[tokio::test]
    async fn marking_one_notification_read_is_scoped_to_the_recipient() {
        let notification_id = Uuid::now_v7();
        let recipient_user_id = Uuid::now_v7();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();

        assert!(
            mark_read(&db, recipient_user_id, notification_id)
                .await
                .unwrap()
        );

        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("UPDATE \\\"user_notifications\\\""));
        assert!(statements.contains("\\\"recipient_user_id\\\" ="));
        assert!(statements.contains(&recipient_user_id.to_string()));
        assert!(statements.contains(&notification_id.to_string()));
    }

    #[tokio::test]
    async fn governance_notification_persists_one_snapshot_per_deduplicated_recipient() {
        let owner = Uuid::now_v7();
        let creator = Uuid::now_v7();
        let actor_user_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let now = OffsetDateTime::UNIX_EPOCH;
        let members = vec![
            team_member::Model {
                team_id,
                ..member(owner, TeamMemberRole::Owner, false)
            },
            team_member::Model {
                team_id,
                ..member(creator, TeamMemberRole::Member, false)
            },
        ];
        let actor = user::Model {
            id: actor_user_id,
            email: "admin@example.com".to_owned(),
            display_name: Some("Platform Admin".to_owned()),
            status: UserStatus::Active,
            platform_role: PlatformRole::Admin,
            last_login_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let eligible_users = [owner, creator]
            .into_iter()
            .map(|id| user::Model {
                id,
                email: format!("{id}@example.invalid"),
                display_name: None,
                status: UserStatus::Active,
                platform_role: PlatformRole::User,
                last_login_at: None,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            })
            .collect::<Vec<_>>();
        let project = project::Model {
            id: project_id,
            team_id,
            created_by_user_id: Some(creator),
            slug: "demo-site".to_owned(),
            name: "Demo".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([members])
            .append_query_results([eligible_users])
            .append_query_results([[actor]])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 2,
            }])
            .into_connection();

        let created = super::create_project_notification(
            &db,
            super::CreateProjectNotification {
                project: &project,
                actor_user_id,
                action: "project.slug_updated",
                reason: Some("Reserved wording".to_owned()),
                target_url: format!("/projects/{project_id}"),
            },
        )
        .await
        .unwrap();

        assert_eq!(created, 2);
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("INSERT INTO \\\"user_notifications\\\""));
        assert!(statements.contains("project.slug_updated"));
        assert!(statements.contains("Reserved wording"));
        assert!(statements.contains("Platform Admin"));
        assert!(statements.contains(&owner.to_string()));
        assert!(statements.contains(&creator.to_string()));
    }
}
