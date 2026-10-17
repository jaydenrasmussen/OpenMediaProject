use omp_db::Db;
use omp_decision::model::DecisionConfig;

use crate::evaluate::ServiceError;

pub const DECISION_CONFIG_KEY: &str = "decision";

/// Settings the decision engine reads. Missing keys take defaults.
pub async fn load_decision_config(db: &Db) -> Result<DecisionConfig, ServiceError> {
    match db.config().get(DECISION_CONFIG_KEY).await? {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(DecisionConfig::default()),
    }
}

pub async fn save_decision_config(db: &Db, config: &DecisionConfig) -> Result<(), ServiceError> {
    db.config()
        .set(DECISION_CONFIG_KEY, &serde_json::to_string(config)?)
        .await?;
    Ok(())
}
