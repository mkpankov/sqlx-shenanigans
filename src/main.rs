use std::env::var;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::error::BoxDynError;
use sqlx::Acquire;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), BoxDynError> {
    println!("Hello, world!");

    let url = var("DATABASE_URL").expect("DATABASE_URL must be specified");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(1))
        .max_lifetime(Duration::from_secs(43200))
        .connect(&url)
        .await
        .expect("Failed to create sqlx database pool");

    let mut bulk_upsert = BulkUpsertQuery::new();

    let rtcs = {
        let mut conn = pool.acquire().await?;

        println!("Executing ListQuery");
        ListQuery::new().execute(&mut conn).await?
    };

    for rtc in rtcs {
        let rtc_id = rtc.id;

        let q = UpsertQuery::new(rtc_id);

        bulk_upsert.query(q);
    }

    let results = {
        let mut conn = pool.acquire().await?;
        let mut trans = conn.begin().await?;
        println!("Executing BulkUpsertQuery");
        bulk_upsert.execute(&mut trans).await?;
        trans.commit().await?;
        // Retrieve state data.
        println!("Executing ListWithRtcQuery");
        ListWithRtcQuery::new().execute(&mut conn).await?
    };

    println!("{results:?}");

    Ok(())
}

struct ListWithRtcRow {
    rtc_id: Id,
    send_audio_updated_by: Option<AgentId>,
}

impl ListWithRtcRow {
    fn split(self) -> (Object, RtcObject) {
        (
            Object {
                rtc_id: self.rtc_id,
                send_audio_updated_by: self.send_audio_updated_by,
            },
            RtcObject { id: self.rtc_id },
        )
    }
}

#[derive(Debug)]
pub struct ListWithRtcQuery {}

impl ListWithRtcQuery {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn execute(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> sqlx::Result<Vec<(Object, RtcObject)>> {
        let results = sqlx::query!(
            r#"
            SELECT
                r.id as "rtc_id: Id",
                rwc.send_audio_updated_by as "send_audio_updated_by?: (Option<AccountId>, Option<String>)"
            FROM rtc_writer_config as rwc
            INNER JOIN rtc as r
            ON rwc.rtc_id = r.id
            "#,
        )
        .fetch_all(conn)
        .await?;

        Ok(results
            .into_iter()
            .map(|r| {
                let send_audio_updated_by = match r.send_audio_updated_by {
                    Some((Some(account_id), Some(label))) => Some(AgentId::new(label, account_id)),
                    _ => None,
                };

                let o = ListWithRtcRow {
                    rtc_id: r.rtc_id,
                    send_audio_updated_by,
                };
                o.split()
            })
            .collect())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RtcObject {
    pub id: Id,
}

#[derive(Default)]
pub struct ListQuery {}

impl ListQuery {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn execute(&self, conn: &mut sqlx::PgConnection) -> sqlx::Result<Vec<RtcObject>> {
        sqlx::query_as!(
            RtcObject,
            r#"
            SELECT
                id as "id: Id"
            FROM rtc
            "#,
        )
        .fetch_all(conn)
        .await
    }
}

#[derive(Clone, Debug)]
pub struct UpsertQuery<'a> {
    rtc_id: Id,
    send_audio_updated_by: Option<&'a AgentId>,
}

impl<'a> UpsertQuery<'a> {
    pub fn new(rtc_id: Id) -> Self {
        Self {
            rtc_id,
            send_audio_updated_by: Default::default(),
        }
    }

    pub fn send_audio_updated_by(self, send_audio_updated_by: &'a AgentId) -> Self {
        Self {
            send_audio_updated_by: Some(send_audio_updated_by),
            ..self
        }
    }
}

#[derive(Clone, Debug)]
pub struct BulkUpsertQuery<'a> {
    queries: Vec<UpsertQuery<'a>>,
}

impl<'a> BulkUpsertQuery<'a> {
    pub fn new() -> Self {
        Self {
            queries: Default::default(),
        }
    }

    pub fn query(&mut self, query: UpsertQuery<'a>) -> &mut Self {
        self.queries.push(query);
        self
    }

    pub async fn execute(&self, conn: &mut sqlx::PgConnection) -> sqlx::Result<Vec<Object>> {
        let rtc_id = self.queries.iter().map(|x| x.rtc_id).collect::<Vec<_>>();

        let send_audio_updated_by = self
            .queries
            .iter()
            .map(|x| x.send_audio_updated_by)
            .collect::<Vec<_>>();

        let results = sqlx::query!(
            r#"
            WITH input(rtc_id,send_audio_updated_by) AS
                (SELECT 
                    rtc_id, 
                    (send_audio_updated_by_account_id, send_audio_updated_by_label)::agent_id
                FROM UNNEST (
                    $1::uuid[], 
                    $2::agent_id[]
                ) AS source(rtc_id,send_audio_updated_by_account_id,send_audio_updated_by_label))
            INSERT INTO 
                rtc_writer_config (
                    rtc_id, 
                    send_audio_updated_by
                )
            (SELECT 
                rtc_id, 
                send_audio_updated_by
            FROM input)
            ON CONFLICT (rtc_id) DO UPDATE
            SET
                send_audio_updated_by = excluded.send_audio_updated_by
            RETURNING
            rtc_id as "rtc_id: Id",
            send_audio_updated_by as "send_audio_updated_by?: (Option<AccountId>, Option<String>)"
            "#,
            &rtc_id as &[Id],
            &send_audio_updated_by as &[Option<&AgentId>],
        )
        .fetch_all(conn)
        .await?;

        Ok(results
            .into_iter()
            .map(|r| {
                let send_audio_updated_by = match r.send_audio_updated_by {
                    Some((Some(account_id), Some(label))) => Some(AgentId::new(label, account_id)),
                    _ => None,
                };

                Object {
                    rtc_id: r.rtc_id,
                    send_audio_updated_by,
                }
            })
            .collect::<Vec<_>>())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId {
    account_id: AccountId,
    label: String,
}

impl sqlx::encode::Encode<'_, sqlx::Postgres> for AgentId
where
    AccountId: for<'q> sqlx::encode::Encode<'q, sqlx::Postgres>,
    AccountId: sqlx::types::Type<sqlx::Postgres>,
    String: for<'q> sqlx::encode::Encode<'q, sqlx::Postgres>,
    String: sqlx::types::Type<sqlx::Postgres>,
{
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> sqlx::encode::IsNull {
        let mut encoder = sqlx::postgres::types::PgRecordEncoder::new(buf);
        encoder.encode(&self.account_id);
        encoder.encode(&self.label);
        encoder.finish();
        sqlx::encode::IsNull::No
    }
    fn size_hint(&self) -> usize {
        2usize * (4 + 4)
            + <AccountId as sqlx::encode::Encode<sqlx::Postgres>>::size_hint(&self.account_id)
            + <String as sqlx::encode::Encode<sqlx::Postgres>>::size_hint(&self.label)
    }
}

// This is what `derive(sqlx::Type)` expands to but with fixed lifetime.
// https://github.com/launchbadge/sqlx/issues/672
impl<'r> sqlx::decode::Decode<'r, sqlx::Postgres> for AgentId
where
    // Originally it was `AccountId: sqlx::decode::Decode<'r, sqlx::Postgres>,`
    AccountId: for<'q> sqlx::decode::Decode<'q, sqlx::Postgres>,
    AccountId: sqlx::types::Type<sqlx::Postgres>,
    String: sqlx::decode::Decode<'r, sqlx::Postgres>,
    String: sqlx::types::Type<sqlx::Postgres>,
{
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + 'static + Send + Sync>> {
        let mut decoder = sqlx::postgres::types::PgRecordDecoder::new(value)?;
        let account_id = decoder.try_decode::<AccountId>()?;
        let label = decoder.try_decode::<String>()?;
        Ok(AgentId { account_id, label })
    }
}

impl sqlx::Type<sqlx::Postgres> for AgentId {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("agent_id")
    }
}

impl sqlx::postgres::PgHasArrayType for AgentId {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        // https://github.com/launchbadge/sqlx/issues/1004#issuecomment-1019438437
        sqlx::postgres::PgTypeInfo::with_name("_agent_id")
    }
}

impl sqlx::postgres::PgHasArrayType for &AgentId {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        // https://github.com/launchbadge/sqlx/issues/1004#issuecomment-1019438437
        sqlx::postgres::PgTypeInfo::with_name("_agent_id")
    }
}

impl AgentId {
    pub fn new<S: Into<String>>(label: S, account_id: AccountId) -> Self {
        Self {
            label: label.into(),
            account_id,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, sqlx::Type)]
#[sqlx(type_name = "account_id")]
pub struct AccountId {
    label: String,
    audience: String,
}

#[derive(Debug)]
pub struct Object {
    #[allow(unused)]
    rtc_id: Id,
    send_audio_updated_by: Option<AgentId>,
}

#[derive(Debug, Deserialize, Serialize, Copy, Clone, Hash, PartialEq, Eq, sqlx::Type)]
#[sqlx(transparent)]
pub struct Id(Uuid);

impl sqlx::postgres::PgHasArrayType for Id {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("_uuid")
    }
}
