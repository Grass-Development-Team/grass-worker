use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QuerySelect, TransactionTrait,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::infra::database::entity::{
    TeamInvitationStatus, TeamKind, TeamMemberRole, team, team_group, team_invitation, team_member,
    user,
};

pub struct CreateTeamParams {
    pub slug: String,
    pub name: String,
    pub kind: TeamKind,
    pub owner_user_id: Uuid,
    pub group_id: Option<Uuid>,
}

pub struct UpdateTeamParams {
    pub slug: Option<String>,
    pub name: Option<String>,
}

pub struct CreateInvitationParams {
    pub team_id: Uuid,
    pub email: String,
    pub role: TeamMemberRole,
    pub invited_by_user_id: Uuid,
    pub token_hash: String,
}

pub struct AcceptInvitationParams {
    pub token_hash: String,
    pub user_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum InvitationError {
    #[error("invitation not found")]
    NotFound,
    #[error("invitation is not pending")]
    NotPending,
    #[error("invitation has expired")]
    Expired,
    #[error("invitation email does not match current user")]
    EmailMismatch,
    #[error("owner role can only be assigned through ownership transfer")]
    OwnerRole,
    #[error("user is already a team member")]
    AlreadyMember,
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

#[derive(Debug, thiserror::Error)]
pub enum MemberMutationError {
    #[error("team member not found")]
    NotFound,
    #[error("{0}")]
    OwnerConflict(String),
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

pub async fn create_team_with_connection<C: ConnectionTrait>(
    db: &C,
    params: CreateTeamParams,
) -> anyhow::Result<team::Model> {
    let now = OffsetDateTime::now_utc();
    let team_id = Uuid::now_v7();

    let group_id = match params.group_id {
        Some(id) => Some(id),
        None => get_default_team_group_id(db).await?,
    };

    let team_model = team::ActiveModel {
        id: Set(team_id),
        slug: Set(params.slug.clone()),
        name: Set(params.name),
        kind: Set(params.kind),
        group_id: Set(group_id),
        explicit_quota_plan_id: Set(None),
        owner_user_id: Set(Some(params.owner_user_id)),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    insert_team_member(
        db,
        team_id,
        params.owner_user_id,
        TeamMemberRole::Owner,
        None,
    )
    .await?;

    Ok(team_model)
}

pub async fn create_team(
    db: &DatabaseConnection,
    params: CreateTeamParams,
) -> anyhow::Result<team::Model> {
    let transaction = db.begin().await?;
    let team = create_team_with_connection(&transaction, params).await?;
    transaction.commit().await?;
    Ok(team)
}

pub async fn list_for_user(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<team::Model>> {
    let memberships = team_member::Entity::find()
        .filter(team_member::Column::UserId.eq(user_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .all(db)
        .await?;

    if memberships.is_empty() {
        return Ok(Vec::new());
    }

    let team_ids = memberships
        .into_iter()
        .map(|m| m.team_id)
        .collect::<Vec<_>>();

    team::Entity::find()
        .filter(team::Column::Id.is_in(team_ids))
        .filter(team::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(Into::into)
}

pub struct TeamListFilter {
    pub query: Option<String>,
    pub limit: u64,
}

/// Platform-wide team listing for administrators, newest first, with an
/// optional case-insensitive slug / name search.
pub async fn list_all(
    db: &DatabaseConnection,
    filter: TeamListFilter,
) -> anyhow::Result<Vec<team::Model>> {
    use sea_orm::QueryOrder;

    let mut query = team::Entity::find().filter(team::Column::DeletedAt.is_null());
    if let Some(term) = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|term| !term.is_empty())
    {
        let pattern = format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        query = query.filter(
            sea_orm::Condition::any()
                .add(team::Column::Slug.like(pattern.clone()))
                .add(team::Column::Name.like(pattern)),
        );
    }
    query
        .order_by_desc(team::Column::CreatedAt)
        .limit(filter.limit.clamp(1, 500))
        .all(db)
        .await
        .map_err(Into::into)
}

/// Live member counts for a set of teams in one grouped query.
pub async fn member_counts(
    db: &DatabaseConnection,
    team_ids: &[Uuid],
) -> anyhow::Result<std::collections::HashMap<Uuid, i64>> {
    use sea_orm::QueryOrder;

    if team_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(Uuid, i64)> = team_member::Entity::find()
        .select_only()
        .column(team_member::Column::TeamId)
        .column_as(team_member::Column::Id.count(), "count")
        .filter(team_member::Column::TeamId.is_in(team_ids.iter().copied()))
        .filter(team_member::Column::DeletedAt.is_null())
        .group_by(team_member::Column::TeamId)
        .order_by_asc(team_member::Column::TeamId)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().collect())
}

pub async fn get_by_id(
    db: &DatabaseConnection,
    team_id: Uuid,
) -> anyhow::Result<Option<team::Model>> {
    team::Entity::find()
        .filter(team::Column::Id.eq(team_id))
        .filter(team::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn update(
    db: &DatabaseConnection,
    team_id: Uuid,
    params: UpdateTeamParams,
) -> anyhow::Result<team::Model> {
    let mut active: team::ActiveModel = get_by_id(db, team_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found"))?
        .into();

    if let Some(slug) = params.slug {
        active.slug = Set(slug);
    }

    if let Some(name) = params.name {
        active.name = Set(name);
    }

    active.update(db).await.map_err(Into::into)
}

#[allow(dead_code)]
pub async fn soft_delete(db: &DatabaseConnection, team_id: Uuid) -> anyhow::Result<()> {
    let team = get_by_id(db, team_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found"))?;
    let mut active: team::ActiveModel = team.into();
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await?;
    Ok(())
}

async fn insert_team_member<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    user_id: Uuid,
    role: TeamMemberRole,
    invited_by: Option<Uuid>,
) -> Result<(), sea_orm::DbErr> {
    let now = OffsetDateTime::now_utc();
    team_member::ActiveModel {
        id: Set(Uuid::now_v7()),
        team_id: Set(team_id),
        user_id: Set(user_id),
        role: Set(role),
        invited_by_user_id: Set(invited_by),
        joined_at: Set(now),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub async fn list_members(
    db: &DatabaseConnection,
    team_id: Uuid,
) -> anyhow::Result<Vec<(team_member::Model, user::Model)>> {
    let members = team_member::Entity::find()
        .filter(team_member::Column::TeamId.eq(team_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .all(db)
        .await?;

    if members.is_empty() {
        return Ok(Vec::new());
    }

    let user_ids = members.iter().map(|m| m.user_id).collect::<Vec<_>>();
    let users = user::Entity::find()
        .filter(user::Column::Id.is_in(user_ids))
        .filter(user::Column::DeletedAt.is_null())
        .all(db)
        .await?;

    Ok(members
        .into_iter()
        .filter_map(|member| {
            users
                .iter()
                .find(|user| user.id == member.user_id)
                .map(|user| (member, user.clone()))
        })
        .collect())
}

pub async fn member_role(
    db: &DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Option<TeamMemberRole>> {
    team_member::Entity::find()
        .filter(team_member::Column::TeamId.eq(team_id))
        .filter(team_member::Column::UserId.eq(user_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|member| member.map(|m| m.role))
        .map_err(Into::into)
}

pub fn invitation_token_hash(token: &str) -> String {
    grass_token::hash_token(token)
}

pub async fn invitation_team_by_token_hash(
    db: &DatabaseConnection,
    token_hash: &str,
) -> anyhow::Result<Option<Uuid>> {
    team_invitation::Entity::find()
        .filter(team_invitation::Column::TokenHash.eq(token_hash))
        .one(db)
        .await
        .map(|invitation| invitation.map(|i| i.team_id))
        .map_err(Into::into)
}

pub async fn invitation_by_token_hash<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
) -> anyhow::Result<Option<team_invitation::Model>> {
    team_invitation::Entity::find()
        .filter(team_invitation::Column::TokenHash.eq(token_hash))
        .one(db)
        .await
        .map_err(Into::into)
}

pub fn validate_invitation_acceptance(
    status: &TeamInvitationStatus,
    expires_at: OffsetDateTime,
    invitation_email: &str,
    user_email: &str,
    now: OffsetDateTime,
) -> Result<(), InvitationError> {
    if !matches!(status, TeamInvitationStatus::Pending) {
        return Err(InvitationError::NotPending);
    }
    if expires_at <= now {
        return Err(InvitationError::Expired);
    }
    if !invitation_email.eq_ignore_ascii_case(user_email) {
        return Err(InvitationError::EmailMismatch);
    }
    Ok(())
}

pub fn validate_managed_member_role(role: &TeamMemberRole) -> anyhow::Result<()> {
    if matches!(role, TeamMemberRole::Owner) {
        anyhow::bail!("owner role can only be assigned through ownership transfer");
    }
    Ok(())
}

pub fn validate_member_change(
    current_role: &TeamMemberRole,
    requested_role: Option<&TeamMemberRole>,
) -> anyhow::Result<()> {
    if matches!(current_role, TeamMemberRole::Owner) {
        anyhow::bail!("team owner cannot be changed through member management");
    }
    if let Some(role) = requested_role {
        validate_managed_member_role(role)?;
    }
    Ok(())
}

pub async fn update_member_role(
    db: &DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
    role: TeamMemberRole,
) -> Result<team_member::Model, MemberMutationError> {
    let transaction = db.begin().await?;
    let member = active_member_for_update(&transaction, team_id, user_id).await?;
    validate_member_change(&member.role, Some(&role))
        .map_err(|error| MemberMutationError::OwnerConflict(error.to_string()))?;
    let mut active: team_member::ActiveModel = member.into();
    active.role = Set(role);
    let member = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(member)
}

pub async fn remove_member(
    db: &DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
) -> Result<(), MemberMutationError> {
    let transaction = db.begin().await?;
    let member = active_member_for_update(&transaction, team_id, user_id).await?;
    validate_member_change(&member.role, None)
        .map_err(|error| MemberMutationError::OwnerConflict(error.to_string()))?;
    let mut active: team_member::ActiveModel = member.into();
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn create_invitation(
    db: &DatabaseConnection,
    params: CreateInvitationParams,
) -> Result<team_invitation::Model, InvitationError> {
    validate_managed_member_role(&params.role).map_err(|_| InvitationError::OwnerRole)?;
    let transaction = db.begin().await?;
    let invited_user = user::Entity::find()
        .filter(user::Column::Email.eq(&params.email))
        .filter(user::Column::DeletedAt.is_null())
        .one(&transaction)
        .await?;

    if let Some(invited_user) = invited_user
        && team_member::Entity::find()
            .filter(team_member::Column::TeamId.eq(params.team_id))
            .filter(team_member::Column::UserId.eq(invited_user.id))
            .filter(team_member::Column::DeletedAt.is_null())
            .one(&transaction)
            .await?
            .is_some()
    {
        return Err(InvitationError::AlreadyMember);
    }

    let now = OffsetDateTime::now_utc();
    let existing = team_invitation::Entity::find()
        .filter(team_invitation::Column::TeamId.eq(params.team_id))
        .filter(team_invitation::Column::Email.eq(&params.email))
        .one(&transaction)
        .await?;

    let invitation = if let Some(existing) = existing {
        let mut active: team_invitation::ActiveModel = existing.into();
        active.role = Set(params.role);
        active.status = Set(TeamInvitationStatus::Pending);
        active.invited_by_user_id = Set(Some(params.invited_by_user_id));
        active.token_hash = Set(Some(params.token_hash));
        active.expires_at = Set(now + Duration::days(7));
        active.accepted_at = Set(None);
        active.update(&transaction).await?
    } else {
        team_invitation::ActiveModel {
            id: Set(Uuid::now_v7()),
            team_id: Set(params.team_id),
            email: Set(params.email),
            role: Set(params.role),
            status: Set(TeamInvitationStatus::Pending),
            invited_by_user_id: Set(Some(params.invited_by_user_id)),
            token_hash: Set(Some(params.token_hash)),
            expires_at: Set(now + Duration::days(7)),
            accepted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&transaction)
        .await?
    };

    transaction.commit().await?;
    Ok(invitation)
}

pub async fn accept_invitation_with_connection<C: ConnectionTrait>(
    db: &C,
    params: AcceptInvitationParams,
) -> Result<team_member::Model, InvitationError> {
    let invitation = team_invitation::Entity::find()
        .filter(team_invitation::Column::TokenHash.eq(&params.token_hash))
        .lock_exclusive()
        .one(db)
        .await?
        .ok_or(InvitationError::NotFound)?;
    let accepting_user = user::Entity::find()
        .filter(user::Column::Id.eq(params.user_id))
        .filter(user::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(InvitationError::NotFound)?;

    validate_invitation_acceptance(
        &invitation.status,
        invitation.expires_at,
        &invitation.email,
        &accepting_user.email,
        OffsetDateTime::now_utc(),
    )?;

    if team_member::Entity::find()
        .filter(team_member::Column::TeamId.eq(invitation.team_id))
        .filter(team_member::Column::UserId.eq(params.user_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some()
    {
        return Err(InvitationError::AlreadyMember);
    }

    let now = OffsetDateTime::now_utc();
    let member = team_member::ActiveModel {
        id: Set(Uuid::now_v7()),
        team_id: Set(invitation.team_id),
        user_id: Set(params.user_id),
        role: Set(invitation.role.clone()),
        invited_by_user_id: Set(invitation.invited_by_user_id),
        joined_at: Set(now),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    let mut active: team_invitation::ActiveModel = invitation.into();
    active.status = Set(TeamInvitationStatus::Accepted);
    active.accepted_at = Set(Some(now));
    active.update(db).await?;

    Ok(member)
}

pub async fn accept_invitation(
    db: &DatabaseConnection,
    params: AcceptInvitationParams,
) -> Result<team_member::Model, InvitationError> {
    let transaction = db.begin().await?;
    let member = accept_invitation_with_connection(&transaction, params).await?;
    transaction.commit().await?;
    Ok(member)
}

async fn active_member_for_update<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    user_id: Uuid,
) -> Result<team_member::Model, MemberMutationError> {
    team_member::Entity::find()
        .filter(team_member::Column::TeamId.eq(team_id))
        .filter(team_member::Column::UserId.eq(user_id))
        .filter(team_member::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(db)
        .await?
        .ok_or(MemberMutationError::NotFound)
}

async fn get_default_team_group_id<C: ConnectionTrait>(db: &C) -> anyhow::Result<Option<Uuid>> {
    team_group::Entity::find()
        .filter(team_group::Column::IsDefault.eq(true))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|g| g.map(|m| m.id))
        .map_err(|e| anyhow::anyhow!("failed to find default team group: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_role_rejects_owner() {
        assert!(validate_managed_member_role(&TeamMemberRole::Owner).is_err());
        assert!(validate_managed_member_role(&TeamMemberRole::Admin).is_ok());
        assert!(validate_managed_member_role(&TeamMemberRole::Member).is_ok());
        assert!(validate_managed_member_role(&TeamMemberRole::Viewer).is_ok());
    }

    #[test]
    fn owner_member_cannot_be_changed_by_member_management() {
        assert!(validate_member_change(&TeamMemberRole::Owner, None).is_err());
        assert!(
            validate_member_change(&TeamMemberRole::Owner, Some(&TeamMemberRole::Admin)).is_err()
        );
        assert!(
            validate_member_change(&TeamMemberRole::Member, Some(&TeamMemberRole::Owner)).is_err()
        );
        assert!(
            validate_member_change(&TeamMemberRole::Member, Some(&TeamMemberRole::Admin)).is_ok()
        );
        assert!(validate_member_change(&TeamMemberRole::Member, None).is_ok());
    }

    #[test]
    fn invitation_acceptance_requires_pending_unexpired_matching_email() {
        let now = OffsetDateTime::now_utc();
        assert!(
            validate_invitation_acceptance(
                &TeamInvitationStatus::Pending,
                now + Duration::minutes(1),
                "invited@example.com",
                "INVITED@example.com",
                now,
            )
            .is_ok()
        );
        assert!(
            validate_invitation_acceptance(
                &TeamInvitationStatus::Accepted,
                now + Duration::minutes(1),
                "invited@example.com",
                "invited@example.com",
                now,
            )
            .is_err()
        );
        assert!(
            validate_invitation_acceptance(
                &TeamInvitationStatus::Pending,
                now,
                "invited@example.com",
                "invited@example.com",
                now,
            )
            .is_err()
        );
        assert!(
            validate_invitation_acceptance(
                &TeamInvitationStatus::Pending,
                now + Duration::minutes(1),
                "invited@example.com",
                "other@example.com",
                now,
            )
            .is_err()
        );
    }

    #[test]
    fn invitation_token_is_hashed_before_storage() {
        let token = "invitation-secret";
        let hash = invitation_token_hash(token);
        assert_ne!(hash, token);
        assert_eq!(hash, grass_token::hash_token(token));
    }
}
