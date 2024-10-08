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

use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};

/// Config for PostgreSQL services support.
#[derive(Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
#[non_exhaustive]
pub struct PostgresqlConfig {
    /// Root of this backend.
    ///
    /// All operations will happen under this root.
    ///
    /// Default to `/` if not set.
    pub root: Option<String>,
    /// the connection string of postgres server
    pub connection_string: Option<String>,
    /// the table of postgresql
    pub table: Option<String>,
    /// the key field of postgresql
    pub key_field: Option<String>,
    /// the value field of postgresql
    pub value_field: Option<String>,
}

impl Debug for PostgresqlConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("PostgresqlConfig");

        if self.connection_string.is_some() {
            d.field("connection_string", &"<redacted>");
        }

        d.field("root", &self.root)
            .field("table", &self.table)
            .field("key_field", &self.key_field)
            .field("value_field", &self.value_field)
            .finish()
    }
}
