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

async fn get_default_team_group_id(db: &DatabaseConnection) -> anyhow::Result<Option<Uuid>> {
    team_group::Entity::find()
        .filter(team_group::Column::IsDefault.eq(true))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|g| g.map(|m| m.id))
        .map_err(|e| anyhow::anyhow!("failed to find default team group: {e}"))
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
