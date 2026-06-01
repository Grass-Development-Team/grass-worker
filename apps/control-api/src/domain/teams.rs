use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{TeamKind, TeamMemberRole, team, team_group, team_member};

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

pub async fn create_team(
    db: &DatabaseConnection,
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

    add_team_member(
        db,
        team_id,
        params.owner_user_id,
        TeamMemberRole::Owner,
        None,
    )
    .await?;

    Ok(team_model)
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

pub async fn add_team_member(
    db: &DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
    role: TeamMemberRole,
    invited_by: Option<Uuid>,
) -> anyhow::Result<()> {
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

async fn get_default_team_group_id(db: &DatabaseConnection) -> anyhow::Result<Option<Uuid>> {
    team_group::Entity::find()
        .filter(team_group::Column::IsDefault.eq(true))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|g| g.map(|m| m.id))
        .map_err(|e| anyhow::anyhow!("failed to find default team group: {e}"))
}
