// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use std::fmt::Debug;
use std::fmt::Formatter;

use rusqlite::params;
use rusqlite::Connection;
use tokio::task;

use crate::raw::adapters::kv;
use crate::raw::*;
use crate::services::SqliteConfig;
use crate::*;

impl Configurator for SqliteConfig {
    type Builder = SqliteBuilder;
    fn into_builder(self) -> Self::Builder {
        SqliteBuilder { config: self }
    }
}

#[doc = include_str!("docs.md")]
#[derive(Default)]
pub struct SqliteBuilder {
    config: SqliteConfig,
}

impl Debug for SqliteBuilder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("SqliteBuilder");

        ds.field("config", &self.config);
        ds.finish()
    }
}

impl SqliteBuilder {
    /// Set the connection_string of the sqlite service.
    ///
    /// This connection string is used to connect to the sqlite service. There are url based formats:
    ///
    /// ## Url
    ///
    /// This format resembles the url format of the sqlite client. The format is: `file://[path]?flag`
    ///
    /// - `file://data.db`
    ///
    /// For more information, please refer to [Opening A New Database Connection](http://www.sqlite.org/c3ref/open.html)
    pub fn connection_string(mut self, v: &str) -> Self {
        if !v.is_empty() {
            self.config.connection_string = Some(v.to_string());
        }
        self
    }

    /// set the working directory, all operations will be performed under it.
    ///
    /// default: "/"
    pub fn root(mut self, root: &str) -> Self {
        self.config.root = if root.is_empty() {
            None
        } else {
            Some(root.to_string())
        };

        self
    }

    /// Set the table name of the sqlite service to read/write.
    pub fn table(mut self, table: &str) -> Self {
        if !table.is_empty() {
            self.config.table = Some(table.to_string());
        }
        self
    }

    /// Set the key field name of the sqlite service to read/write.
    ///
    /// Default to `key` if not specified.
    pub fn key_field(mut self, key_field: &str) -> Self {
        if !key_field.is_empty() {
            self.config.key_field = Some(key_field.to_string());
        }
        self
    }

    /// Set the value field name of the sqlite service to read/write.
    ///
    /// Default to `value` if not specified.
    pub fn value_field(mut self, value_field: &str) -> Self {
        if !value_field.is_empty() {
            self.config.value_field = Some(value_field.to_string());
        }
        self
    }
}

impl Builder for SqliteBuilder {
    const SCHEME: Scheme = Scheme::Sqlite;
    type Config = SqliteConfig;

    fn build(self) -> Result<impl Access> {
        let connection_string = match self.config.connection_string.clone() {
            Some(v) => v,
            None => {
                return Err(Error::new(
                    ErrorKind::ConfigInvalid,
                    "connection_string is required but not set",
                )
                .with_context("service", Scheme::Sqlite));
            }
        };
        let table = match self.config.table.clone() {
            Some(v) => v,
            None => {
                return Err(Error::new(ErrorKind::ConfigInvalid, "table is empty")
                    .with_context("service", Scheme::Sqlite));
            }
        };
        let key_field = match self.config.key_field.clone() {
            Some(v) => v,
            None => "key".to_string(),
        };
        let value_field = match self.config.value_field.clone() {
            Some(v) => v,
            None => "value".to_string(),
        };
        let root = normalize_root(
            self.config
                .root
                .clone()
                .unwrap_or_else(|| "/".to_string())
                .as_str(),
        );
        let mgr = SqliteConnectionManager { connection_string };
        let pool = r2d2::Pool::new(mgr).map_err(|err| {
            Error::new(ErrorKind::Unexpected, "sqlite pool init failed").set_source(err)
        })?;

        Ok(SqliteBackend::new(Adapter {
            pool,
            table,
            key_field,
            value_field,
        })
        .with_normalized_root(root))
    }
}

struct SqliteConnectionManager {
    connection_string: String,
}

impl r2d2::ManageConnection for SqliteConnectionManager {
    type Connection = Connection;
    type Error = Error;

    fn connect(&self) -> Result<Connection> {
        Connection::open(&self.connection_string)
            .map_err(|err| Error::new(ErrorKind::Unexpected, "sqlite open error").set_source(err))
    }

    fn is_valid(&self, conn: &mut Connection) -> Result<()> {
        conn.execute_batch("").map_err(parse_rusqlite_error)
    }

    fn has_broken(&self, _: &mut Connection) -> bool {
        false
    }
}

pub type SqliteBackend = kv::Backend<Adapter>;

#[derive(Clone)]
pub struct Adapter {
    pool: r2d2::Pool<SqliteConnectionManager>,

    table: String,
    key_field: String,
    value_field: String,
}

impl Debug for Adapter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("SqliteAdapter");
        ds.field("table", &self.table);
        ds.field("key_field", &self.key_field);
        ds.field("value_field", &self.value_field);
        ds.finish()
    }
}

impl kv::Adapter for Adapter {
    fn metadata(&self) -> kv::Metadata {
        kv::Metadata::new(
            Scheme::Sqlite,
            &self.table,
            Capability {
                read: true,
                write: true,
                delete: true,
                blocking: true,
                list: true,
                ..Default::default()
            },
        )
    }

    async fn get(&self, path: &str) -> Result<Option<Buffer>> {
        let this = self.clone();
        let path = path.to_string();

        task::spawn_blocking(move || this.blocking_get(&path))
            .await
            .map_err(new_task_join_error)?
    }

    fn blocking_get(&self, path: &str) -> Result<Option<Buffer>> {
        let conn = self.pool.get().map_err(parse_r2d2_error)?;

        let query = format!(
            "SELECT {} FROM {} WHERE `{}` = $1 LIMIT 1",
            self.value_field, self.table, self.key_field
        );
        let mut statement = conn.prepare(&query).map_err(parse_rusqlite_error)?;
        let result: rusqlite::Result<Vec<u8>> = statement.query_row([path], |row| row.get(0));
        match result {
            Ok(v) => Ok(Some(Buffer::from(v))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(parse_rusqlite_error(err)),
        }
    }

    async fn set(&self, path: &str, value: Buffer) -> Result<()> {
        let this = self.clone();
        let path = path.to_string();
        task::spawn_blocking(move || this.blocking_set(&path, value))
            .await
            .map_err(new_task_join_error)?
    }

    fn blocking_set(&self, path: &str, value: Buffer) -> Result<()> {
        let conn = self.pool.get().map_err(parse_r2d2_error)?;

        let query = format!(
            "INSERT OR REPLACE INTO `{}` (`{}`, `{}`) VALUES ($1, $2)",
            self.table, self.key_field, self.value_field
        );
        let mut statement = conn.prepare(&query).map_err(parse_rusqlite_error)?;
        statement
            .execute(params![path, value.to_vec()])
            .map_err(parse_rusqlite_error)?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let this = self.clone();
        let path = path.to_string();

        task::spawn_blocking(move || this.blocking_delete(&path))
            .await
            .map_err(new_task_join_error)?
    }

    fn blocking_delete(&self, path: &str) -> Result<()> {
        let conn = self.pool.get().map_err(parse_r2d2_error)?;

        let query = format!("DELETE FROM {} WHERE `{}` = $1", self.table, self.key_field);
        let mut statement = conn.prepare(&query).map_err(parse_rusqlite_error)?;
        statement.execute([path]).map_err(parse_rusqlite_error)?;
        Ok(())
    }

    async fn scan(&self, path: &str) -> Result<Vec<String>> {
        let this = self.clone();
        let path = path.to_string();

        task::spawn_blocking(move || this.blocking_scan(&path))
            .await
            .map_err(new_task_join_error)?
    }

    fn blocking_scan(&self, path: &str) -> Result<Vec<String>> {
        let conn = self.pool.get().map_err(parse_r2d2_error)?;
        let query = format!(
            "SELECT {} FROM {} WHERE `{}` LIKE $1 and `{}` <> $2",
            self.key_field, self.table, self.key_field, self.key_field
        );
        let mut statement = conn.prepare(&query).map_err(parse_rusqlite_error)?;
        let like_param = format!("{}%", path);
        let result = statement.query(params![like_param, path]);

        match result {
            Ok(mut rows) => {
                let mut keys: Vec<String> = Vec::new();
                while let Some(row) = rows.next().map_err(parse_rusqlite_error)? {
                    let item = row.get(0).map_err(parse_rusqlite_error)?;
                    keys.push(item);
                }
                Ok(keys)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(vec![]),
            Err(err) => Err(parse_rusqlite_error(err)),
        }
    }
}

fn parse_rusqlite_error(err: rusqlite::Error) -> Error {
    Error::new(ErrorKind::Unexpected, "unhandled error from sqlite").set_source(err)
}

fn parse_r2d2_error(err: r2d2::Error) -> Error {
    Error::new(ErrorKind::Unexpected, "unhandled error from r2d2").set_source(err)
}
