//! Fuzz/边界属性测试（TODO 9.3.2）：确保任意字节/任意 JSON 不会导致 panic。
//!
//! - HTTP handler 请求类型：serde 反序列化不 panic
//! - gRPC/prost payload：decode 不 panic

use proptest::prelude::*;
use prost::Message;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn http_goal_execute_request_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::GoalExecuteRequest>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn http_composite_execute_request_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::CompositeExecuteRequest>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn http_team_execute_request_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::TeamExecuteRequest>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn http_install_skill_request_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::InstallSkillRequest>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn http_sop_save_request_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::SopSaveRequest>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn http_cognitive_trace_query_never_panics_on_arbitrary_json(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let r = serde_json::from_slice::<sisyphus_bridge::http::types::CognitiveTraceQuery>(&data);
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn grpc_join_request_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let r = sisyphus_bridge_api::cluster_proto::JoinRequest::decode(data.as_slice());
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn grpc_cluster_event_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let r = sisyphus_bridge_api::cluster_proto::ClusterEvent::decode(data.as_slice());
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn grpc_experience_sync_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let r = sisyphus_bridge_api::cluster_proto::ExperienceSync::decode(data.as_slice());
        prop_assert!(r.is_ok() || r.is_err());
    }

    #[test]
    fn grpc_sync_request_decode_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let r = sisyphus_bridge_api::cluster_proto::SyncRequest::decode(data.as_slice());
        prop_assert!(r.is_ok() || r.is_err());
    }
}
