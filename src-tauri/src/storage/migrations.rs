use rusqlite::Connection;

use super::StorageError;

type MigrationAction = fn(&Connection) -> Result<(), StorageError>;

#[derive(Clone, Copy)]
struct Migration {
    to_version: i64,
    description: &'static str,
    apply: MigrationAction,
}

// Schema v1 is the clean migration baseline. Future schema changes must bump
// SCHEMA_VERSION and register exactly one adjacent migration here. There are
// intentionally no historical/pre-v1 readers or placeholder migrations.
const REGISTERED_MIGRATIONS: &[Migration] = &[];

pub(super) fn migrate_to_current(
    connection: &Connection,
    from_version: i64,
    target_version: i64,
    finalize_schema: MigrationAction,
) -> Result<(), StorageError> {
    run_migration_chain(
        connection,
        from_version,
        target_version,
        REGISTERED_MIGRATIONS,
        finalize_schema,
    )
}

pub(super) fn validate_registered_plan(
    from_version: i64,
    target_version: i64,
) -> Result<(), StorageError> {
    if from_version == target_version {
        return Ok(());
    }
    resolve_migration_chain(from_version, target_version, REGISTERED_MIGRATIONS).map(|_| ())
}

fn run_migration_chain(
    connection: &Connection,
    from_version: i64,
    target_version: i64,
    migrations: &[Migration],
    finalize_schema: MigrationAction,
) -> Result<(), StorageError> {
    let chain = resolve_migration_chain(from_version, target_version, migrations)?;

    connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    if let Err(error) = connection.execute_batch("BEGIN IMMEDIATE;") {
        let _ = connection.execute_batch("PRAGMA foreign_keys = ON;");
        return Err(error.into());
    }

    let result = (|| {
        for migration in chain {
            (migration.apply)(connection).map_err(|error| {
                StorageError::DatabaseMigrationFailed {
                    from_version: migration.to_version - 1,
                    to_version: migration.to_version,
                    reason: format!("{}: {error}", migration.description),
                }
            })?;
            connection.pragma_update(None, "user_version", migration.to_version)?;
        }

        finalize_schema(connection).map_err(|error| StorageError::DatabaseMigrationFailed {
            from_version,
            to_version: target_version,
            reason: format!("final schema validation failed: {error}"),
        })?;
        ensure_foreign_keys_are_valid(connection, from_version, target_version)?;
        Ok::<(), StorageError>(())
    })();

    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT; PRAGMA foreign_keys = ON;")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK; PRAGMA foreign_keys = ON;");
            Err(error)
        }
    }
}

fn resolve_migration_chain(
    from_version: i64,
    target_version: i64,
    migrations: &[Migration],
) -> Result<Vec<Migration>, StorageError> {
    if from_version >= target_version {
        return Err(StorageError::InvalidDatabaseMigrationPlan {
            from_version,
            to_version: target_version,
            reason: "migration target must be newer than the source".to_string(),
        });
    }

    let mut chain = Vec::new();
    for next_version in (from_version + 1)..=target_version {
        let matches = migrations
            .iter()
            .filter(|migration| migration.to_version == next_version)
            .copied()
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(StorageError::InvalidDatabaseMigrationPlan {
                from_version,
                to_version: target_version,
                reason: format!(
                    "expected exactly one migration from v{} to v{next_version}, found {}",
                    next_version - 1,
                    matches.len()
                ),
            });
        }
        chain.push(matches[0]);
    }
    Ok(chain)
}

fn ensure_foreign_keys_are_valid(
    connection: &Connection,
    from_version: i64,
    target_version: i64,
) -> Result<(), StorageError> {
    let violation = {
        let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        rows.next()?
            .map(|row| {
                Ok::<_, rusqlite::Error>((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .transpose()?
    };
    if let Some((table, row_id, parent)) = violation {
        return Err(StorageError::DatabaseMigrationFailed {
            from_version,
            to_version: target_version,
            reason: format!(
                "foreign-key check failed for table {table}, row {}, parent {parent}",
                row_id.map_or_else(|| "unknown".to_string(), |value| value.to_string())
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migration(to_version: i64, description: &'static str, apply: MigrationAction) -> Migration {
        Migration {
            to_version,
            description,
            apply,
        }
    }

    fn setup_version_one(connection: &Connection) {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE records (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO records (id, value) VALUES (1, 'preserved');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    }

    fn migrate_to_two(connection: &Connection) -> Result<(), StorageError> {
        connection.execute_batch(
            "ALTER TABLE records ADD COLUMN note TEXT NOT NULL DEFAULT '';
             UPDATE records SET note = 'v2';",
        )?;
        Ok(())
    }

    fn migrate_to_three(connection: &Connection) -> Result<(), StorageError> {
        connection.execute_batch(
            "CREATE TABLE migration_audit (version INTEGER PRIMARY KEY);
             INSERT INTO migration_audit VALUES (3);",
        )?;
        Ok(())
    }

    fn fail_migration(connection: &Connection) -> Result<(), StorageError> {
        connection.execute_batch("INSERT INTO missing_table VALUES (3);")?;
        Ok(())
    }

    fn finalize(connection: &Connection) -> Result<(), StorageError> {
        connection.execute_batch("CREATE INDEX records_value_idx ON records (value);")?;
        Ok(())
    }

    #[test]
    fn sequential_migrations_preserve_rows_and_advance_the_version() {
        let connection = Connection::open_in_memory().unwrap();
        setup_version_one(&connection);
        let migrations = [
            migration(2, "add record note", migrate_to_two),
            migration(3, "add migration audit", migrate_to_three),
        ];

        run_migration_chain(&connection, 1, 3, &migrations, finalize).unwrap();

        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let row: (String, String) = connection
            .query_row("SELECT value, note FROM records WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(version, 3);
        assert_eq!(row, ("preserved".to_string(), "v2".to_string()));
        assert_eq!(
            connection
                .query_row("SELECT version FROM migration_audit", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
    }

    #[test]
    fn later_failure_rolls_back_the_entire_migration_chain() {
        let connection = Connection::open_in_memory().unwrap();
        setup_version_one(&connection);
        let migrations = [
            migration(2, "add record note", migrate_to_two),
            migration(3, "fail deliberately", fail_migration),
        ];

        assert!(run_migration_chain(&connection, 1, 3, &migrations, finalize).is_err());

        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(records)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let foreign_keys: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        assert_eq!(columns, ["id", "value"]);
        assert_eq!(foreign_keys, 1);
        assert_eq!(
            connection
                .query_row("SELECT value FROM records WHERE id = 1", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "preserved"
        );
    }

    #[test]
    fn migration_plan_must_cover_every_adjacent_version_once() {
        let migrations = [migration(3, "skip v2", migrate_to_three)];

        assert!(matches!(
            resolve_migration_chain(1, 3, &migrations),
            Err(StorageError::InvalidDatabaseMigrationPlan { .. })
        ));
    }
}
