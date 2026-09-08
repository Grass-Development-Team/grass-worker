use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "regional_ingress_health")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub ingress_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub node_id: Uuid,
    pub status: String,
    pub checked_at: Option<TimeDateTimeWithTimeZone>,
    pub latency_ms: Option<i32>,
    pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::regional_ingress::Entity",
        from = "Column::IngressId",
        to = "super::regional_ingress::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Ingress,
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::NodeId",
        to = "super::node::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Node,
}

impl Related<super::regional_ingress::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Ingress.def()
    }
}

impl Related<super::node::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Node.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
