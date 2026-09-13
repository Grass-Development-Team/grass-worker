use std::{ops::Deref, sync::Mutex};

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, ExecResult,
    QueryResult, Statement, TransactionTrait,
};

use crate::infra::database::entity::audit_event;

/// A connection that knows when a successfully inserted audit becomes durable.
/// Raw transactions deliberately do not implement this: their owner must use
/// `AuditTransaction` so a rolled-back event cannot produce a success log.
pub trait AuditConnection: ConnectionTrait {
    fn audit_written(&self, event: audit_event::Model);
}

impl AuditConnection for DatabaseConnection {
    fn audit_written(&self, event: audit_event::Model) {
        super::logging::committed(&event);
    }
}

/// Holds audit logs until the database transaction successfully commits.
/// Dropping or rolling back this value discards its pending logs.
#[must_use = "the transaction must be committed or rolled back"]
pub struct AuditTransaction {
    inner: DatabaseTransaction,
    pending: Mutex<Vec<audit_event::Model>>,
}

impl AuditTransaction {
    pub async fn begin(db: &DatabaseConnection) -> Result<Self, DbErr> {
        Ok(Self {
            inner: db.begin().await?,
            pending: Mutex::new(Vec::new()),
        })
    }

    pub async fn commit(self) -> Result<(), DbErr> {
        self.inner.commit().await?;
        for event in self
            .pending
            .into_inner()
            .expect("audit buffer lock poisoned")
        {
            super::logging::committed(&event);
        }
        Ok(())
    }

    pub async fn rollback(self) -> Result<(), DbErr> {
        self.inner.rollback().await
    }
}

impl Deref for AuditTransaction {
    type Target = DatabaseTransaction;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AuditConnection for AuditTransaction {
    fn audit_written(&self, event: audit_event::Model) {
        self.pending
            .lock()
            .expect("audit buffer lock poisoned")
            .push(event);
    }
}

#[async_trait::async_trait]
impl ConnectionTrait for AuditTransaction {
    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    async fn execute_raw(&self, statement: Statement) -> Result<ExecResult, DbErr> {
        self.inner.execute_raw(statement).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        self.inner.execute_unprepared(sql).await
    }

    async fn query_one_raw(&self, statement: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.inner.query_one_raw(statement).await
    }

    async fn query_all_raw(&self, statement: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.inner.query_all_raw(statement).await
    }

    fn is_mock_connection(&self) -> bool {
        self.inner.is_mock_connection()
    }
}
