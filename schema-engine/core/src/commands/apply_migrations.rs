use crate::{json_rpc::types::*, CoreError, CoreResult};
use schema_connector::{
    migrations_directory::{error_on_changed_provider, list_migrations, MigrationDirectory},
    ConnectorError, MigrationRecord, Namespaces, PersistenceNotInitializedError, SchemaConnector,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use tracing::Instrument;
use user_facing_errors::schema_engine::FoundFailedMigrations;

use std::fs;
use std::process::Command;

pub fn super_log(identifier: &String, data: &str) {
    let mut path = "/dev/tty";
    if cfg!(windows) {
        path = "CON:";
    }
    fs::write(path, format!("[{}] ", identifier)).expect("Unable to write file");
    fs::write(path, data).expect("Unable to write file");
    fs::write(path, "\n").expect("Unable to write file");
}

fn script_execute(identifier: String, script: PathBuf, prisma: PathBuf) -> Option<ConnectorError> {
    if script.exists() {
        super_log(&identifier, "Custom migration script detected");
        if prisma.exists() {
            super_log(&identifier, "Schema state detected, generating...");
            let generate = Command::new("yarn")
                .arg("prisma")
                .arg("generate")
                .arg("--schema")
                .arg(prisma.as_os_str())
                .output()
                .expect("Failed to generate");
            if generate.status.success() {
                super_log(&identifier, "Generated successfully")
            } else {
                return Some(ConnectorError::from_msg(format!(
                    "[{}] Error in generating.\nSTDOUT: {}\nSTDERR: {}\n",
                    identifier,
                    String::from_utf8_lossy(&generate.stdout),
                    String::from_utf8_lossy(&generate.stderr),
                )));
            }
        }

        super_log(&identifier, "Executing script");
        let yarn = Command::new("yarn")
            .arg("ts-node")
            .arg(script.as_os_str())
            .output()
            .expect("Failed to execute");

        if yarn.status.success() {
            super_log(
                &identifier,
                &format!(
                    "Script executed successfully. STDOUT:\n{}",
                    String::from_utf8_lossy(&yarn.stdout)
                )
                .to_string(),
            );
        } else {
            return Some(ConnectorError::from_msg(format!(
                "[{}] Error while executing.\nSTDOUT: {}\nSTDERR: {}\n",
                identifier,
                String::from_utf8_lossy(&yarn.stdout),
                String::from_utf8_lossy(&yarn.stderr),
            )));
        }
    }
    return None;
}

pub async fn apply_migrations(
    input: ApplyMigrationsInput,
    connector: &mut dyn SchemaConnector,
    namespaces: Option<Namespaces>,
) -> CoreResult<ApplyMigrationsOutput> {
    let start = Instant::now();

    error_on_changed_provider(&input.migrations_directory_path, connector.connector_type())?;

    connector.acquire_lock().await?;
    connector.migration_persistence().initialize(namespaces).await?;

    let migrations_from_filesystem = list_migrations(Path::new(&input.migrations_directory_path))?;
    let migrations_from_database = connector
        .migration_persistence()
        .list_migrations()
        .await?
        .map_err(PersistenceNotInitializedError::into_connector_error)?;

    detect_failed_migrations(&migrations_from_database)?;

    // We are now on the Happy Path™.
    tracing::debug!("Migration history is OK, applying unapplied migrations.");
    let unapplied_migrations: Vec<&MigrationDirectory> = migrations_from_filesystem
        .iter()
        .filter(|fs_migration| {
            !migrations_from_database
                .iter()
                .filter(|db_migration| db_migration.rolled_back_at.is_none())
                .any(|db_migration| fs_migration.migration_name() == db_migration.migration_name)
        })
        .collect();

    let analysis_duration_ms = Instant::now().duration_since(start).as_millis() as u64;
    tracing::info!(analysis_duration_ms, "Analysis run in {}ms", analysis_duration_ms,);

    let mut applied_migration_names: Vec<String> = Vec::with_capacity(unapplied_migrations.len());

    for unapplied_migration in unapplied_migrations {
        let fut = async {
            let script = unapplied_migration
                .read_migration_script()
                .map_err(ConnectorError::from)?;

            let directory = unapplied_migration.path();
            let before_script = directory.join("before.ts");
            let before_prisma = directory.join("before.prisma");
            let after_script = directory.join("after.ts");
            let after_prisma = directory.join("after.prisma");

            let migration_id = connector
                .migration_persistence()
                .record_migration_started(unapplied_migration.migration_name(), &script)
                .await?;

            async fn fail(connector: &mut dyn SchemaConnector, migration_id: &String, logs: String) {
                let _ = connector
                    .migration_persistence()
                    .record_failed_step(&migration_id, &logs)
                    .await;
            }

            let before = script_execute("BEFORE".to_string(), before_script, before_prisma);
            if before.is_some() {
                let error = before.unwrap();
                fail(connector, &migration_id, error.to_string()).await;
                return Err(error);
            }

            tracing::info!(
                script = script.as_str(),
                "Applying `{}`",
                unapplied_migration.migration_name()
            );

            match connector
                .apply_script(unapplied_migration.migration_name(), &script)
                .await
            {
                Ok(()) => {
                    tracing::debug!("Successfully applied the script.");
                    let after = script_execute("AFTER".to_string(), after_script, after_prisma);

                    if after.is_some() {
                        let error = after.unwrap();
                        fail(connector, &migration_id, error.to_string()).await;
                        return Err(error);
                    }

                    let p = connector.migration_persistence();
                    p.record_successful_step(&migration_id).await?;
                    p.record_migration_finished(&migration_id).await?;
                    applied_migration_names.push(unapplied_migration.migration_name().to_owned());

                    return Ok(());
                }
                Err(err) => {
                    tracing::debug!("Failed to apply the script.");

                    let logs = err.to_string();
                    fail(connector, &migration_id, logs).await;

                    return Err(err);
                }
            }
        };
        fut.instrument(tracing::info_span!(
            "Applying migration",
            migration_name = unapplied_migration.migration_name(),
        ))
        .await?
    }

    Ok(ApplyMigrationsOutput {
        applied_migration_names,
    })
}

fn detect_failed_migrations(migrations_from_database: &[MigrationRecord]) -> CoreResult<()> {
    use std::fmt::Write as _;

    tracing::debug!("Checking for failed migrations.");

    let mut failed_migrations = migrations_from_database
        .iter()
        .filter(|migration| migration.finished_at.is_none() && migration.rolled_back_at.is_none())
        .peekable();

    if failed_migrations.peek().is_none() {
        return Ok(());
    }

    let mut details = String::new();

    for failed_migration in failed_migrations {
        let logs = failed_migration
            .logs
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!(" with the following logs:\n{s}"))
            .unwrap_or_default();

        writeln!(
            details,
            "The `{name}` migration started at {started_at} failed{logs}",
            name = failed_migration.migration_name,
            started_at = failed_migration.started_at,
        )
        .unwrap();
    }

    Err(CoreError::user_facing(FoundFailedMigrations { details }))
}
