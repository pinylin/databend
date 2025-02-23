// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use common_base::tokio;
use common_meta_api::KVApiTestSuite;

use crate::init_meta_ut;
use crate::tests::service::start_metasrv_cluster;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_kv_api_write_read_cross_nodes() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_meta_ut!();
    let _ent = ut_span.enter();

    let tcs = start_metasrv_cluster(&[0, 1, 2]).await?;

    let follower1 = tcs[1].flight_client().await?;
    let follower2 = tcs[2].flight_client().await?;

    KVApiTestSuite {}
        .kv_write_read_cross_nodes(&follower1, &follower2)
        .await
}
