//! Encapsulation for setting up external services.

use crate::{
    config::Config,
    core::Alert,
    notification::{manager::NotificationManager, slack::SlackClient},
};
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::info;

/// Sets up the Slack notification pipeline if it is enabled in the configuration.
///
/// Returns a `broadcast::Sender<Alert>` if the pipeline is successfully started,
/// otherwise returns `Ok(None)`.
pub fn setup_notification_pipeline(config: &Config) -> Result<Option<broadcast::Sender<Alert>>> {
    if let Some(slack_config) = &config.output.slack {
        if slack_config.enabled {
            if slack_config.webhook_url.is_empty() {
                tracing::warn!("Slack notifications are enabled, but no webhook URL was provided. Slack notifications will be disabled.");
                return Ok(None);
            }

            let (tx, _rx) = broadcast::channel::<Alert>(config.performance.queue_capacity);
            info!("Slack notification pipeline enabled.");

            // Spawn the Slack notifier.
            let slack_config = slack_config.clone();
            let slack_formatter = Box::new(crate::formatting::SlackTextFormatter);
            let slack_client = std::sync::Arc::new(SlackClient::new(
                slack_config.webhook_url.clone(),
                slack_formatter,
            ));
            let slack_notifier =
                NotificationManager::new(slack_config, tx.subscribe(), slack_client);
            tokio::spawn(slack_notifier.run());
            return Ok(Some(tx));
        }
    }
    Ok(None)
}