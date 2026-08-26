//! gRPC 处理器：`SisyphusBridge` trait 实现。
//!
//! 方法体按域拆分为 [`crate::grpc`] 子模块（`execution` / `manage` / `misc`），
//! 此处仅保留 trait 实现薄壳与流式关联类型。

#![allow(clippy::result_large_err)]

use sisyphus_bridge_api::sisyphus_bridge_server::SisyphusBridge;
use sisyphus_bridge_api::{
    AddMessageRequest,
    AddMessageResponse,
    BreakerMetricsRequest,
    BreakerMetricsResponse,
    CaptureExperienceRequest,
    CaptureExperienceResponse,
    CognitiveTaskEvent,
    CreateWorkspaceRequest,
    CreateWorkspaceResponse,
    DecomposeGoalRequest,
    DecomposeGoalResponse,
    DelegateCognitiveTaskRequest,
    DelegateCognitiveTaskResponse,
    DeleteWorkspaceRequest,
    DeleteWorkspaceResponse,
    ExecuteSopRequest,
    ExecuteToolRequest,
    ExecuteToolResponse,
    ExecutionEvent,
    GetCurrentWorkspaceRequest,
    GetCurrentWorkspaceResponse,
    HealthCheckRequest,
    HealthCheckResponse,
    InstallSkillRequest,
    InstallSkillResponse,
    ListAgentsRequest,
    ListAgentsResponse,
    ListBenchmarksRequest,
    ListBenchmarksResponse,
    ListPluginsRequest,
    ListPluginsResponse,
    // Experience sharing
    ListSharedExperiencesRequest,
    ListSharedExperiencesResponse,
    ListSkillsRequest,
    ListSkillsResponse,
    ListSoPsRequest,
    ListSoPsResponse,
    ListToolsRequest,
    ListToolsResponse,
    ListWorkspacesRequest,
    ListWorkspacesResponse,
    LoadPluginRequest,
    LoadPluginResponse,
    PullExperienceRequest,
    PullExperienceResponse,
    RemoveSkillRequest,
    RemoveSkillResponse,
    ShareExperienceRequest,
    ShareExperienceResponse,
    ShowSkillRequest,
    ShowSkillResponse,
    SopResponse,
    SubmitFeedbackRequest,
    SubmitFeedbackResponse,
    SwitchWorkspaceRequest,
    SwitchWorkspaceResponse,
    UnloadPluginRequest,
    UnloadPluginResponse,
};

use tonic::{Request, Response, Status};

use super::MySisyphusBridge;

#[tonic::async_trait]
impl SisyphusBridge for MySisyphusBridge {
    type StreamEventsStream = tonic::codegen::BoxStream<sisyphus_bridge_api::BridgeEvent>;
    type StreamExecutionStream = tonic::codegen::BoxStream<ExecutionEvent>;
    type StreamCognitiveTaskStream = tonic::codegen::BoxStream<CognitiveTaskEvent>;
    type StreamAnomalyReportStream = tonic::codegen::BoxStream<sisyphus_bridge_api::AnomalyReport>;

    async fn execute_sop(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<SopResponse>, Status> {
        self.execute_sop_impl(request).await
    }

    async fn decompose_goal(
        &self,
        request: Request<DecomposeGoalRequest>,
    ) -> Result<Response<DecomposeGoalResponse>, Status> {
        self.decompose_goal_impl(request).await
    }

    async fn execute_tool(
        &self,
        request: Request<ExecuteToolRequest>,
    ) -> Result<Response<ExecuteToolResponse>, Status> {
        self.execute_tool_impl(request).await
    }

    async fn stream_execution(
        &self,
        request: Request<ExecuteSopRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<ExecutionEvent>>, Status> {
        self.stream_execution_impl(request).await
    }

    async fn stream_cognitive_task(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<CognitiveTaskEvent>>, Status> {
        self.stream_cognitive_task_impl(request).await
    }

    async fn delegate_cognitive_task(
        &self,
        request: Request<DelegateCognitiveTaskRequest>,
    ) -> Result<Response<DelegateCognitiveTaskResponse>, Status> {
        self.delegate_cognitive_task_impl(request).await
    }

    async fn add_message(
        &self,
        request: Request<AddMessageRequest>,
    ) -> Result<Response<AddMessageResponse>, Status> {
        self.add_message_impl(request).await
    }

    async fn call_jcode(
        &self,
        request: Request<sisyphus_bridge_api::CallJcodeRequest>,
    ) -> Result<Response<sisyphus_bridge_api::CallJcodeResponse>, Status> {
        self.call_jcode_impl(request).await
    }

    async fn list_agents(
        &self,
        _request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        self.list_agents_impl(_request).await
    }

    async fn list_tools(
        &self,
        _request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        self.list_tools_impl(_request).await
    }

    async fn list_so_ps(
        &self,
        _request: Request<ListSoPsRequest>,
    ) -> Result<Response<ListSoPsResponse>, Status> {
        self.list_so_ps_impl(_request).await
    }

    async fn list_skills(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        self.list_skills_impl(request).await
    }

    async fn show_skill(
        &self,
        request: Request<ShowSkillRequest>,
    ) -> Result<Response<ShowSkillResponse>, Status> {
        self.show_skill_impl(request).await
    }

    async fn install_skill(
        &self,
        request: Request<InstallSkillRequest>,
    ) -> Result<Response<InstallSkillResponse>, Status> {
        self.install_skill_impl(request).await
    }

    async fn remove_skill(
        &self,
        request: Request<RemoveSkillRequest>,
    ) -> Result<Response<RemoveSkillResponse>, Status> {
        self.remove_skill_impl(request).await
    }

    async fn list_plugins(
        &self,
        _request: Request<ListPluginsRequest>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        self.list_plugins_impl(_request).await
    }

    async fn load_plugin(
        &self,
        request: Request<LoadPluginRequest>,
    ) -> Result<Response<LoadPluginResponse>, Status> {
        self.load_plugin_impl(request).await
    }

    async fn unload_plugin(
        &self,
        request: Request<UnloadPluginRequest>,
    ) -> Result<Response<UnloadPluginResponse>, Status> {
        self.unload_plugin_impl(request).await
    }

    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceRequest>,
    ) -> Result<Response<CreateWorkspaceResponse>, Status> {
        self.create_workspace_impl(request).await
    }

    async fn switch_workspace(
        &self,
        request: Request<SwitchWorkspaceRequest>,
    ) -> Result<Response<SwitchWorkspaceResponse>, Status> {
        self.switch_workspace_impl(request).await
    }

    async fn list_workspaces(
        &self,
        _request: Request<ListWorkspacesRequest>,
    ) -> Result<Response<ListWorkspacesResponse>, Status> {
        self.list_workspaces_impl(_request).await
    }

    async fn get_current_workspace(
        &self,
        _request: Request<GetCurrentWorkspaceRequest>,
    ) -> Result<Response<GetCurrentWorkspaceResponse>, Status> {
        self.get_current_workspace_impl(_request).await
    }

    async fn delete_workspace(
        &self,
        request: Request<DeleteWorkspaceRequest>,
    ) -> Result<Response<DeleteWorkspaceResponse>, Status> {
        self.delete_workspace_impl(request).await
    }

    async fn health_check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        self.health_check_impl(_request).await
    }

    async fn submit_feedback(
        &self,
        request: Request<SubmitFeedbackRequest>,
    ) -> Result<Response<SubmitFeedbackResponse>, Status> {
        self.submit_feedback_impl(request).await
    }

    async fn capture_experience(
        &self,
        request: Request<CaptureExperienceRequest>,
    ) -> Result<Response<CaptureExperienceResponse>, Status> {
        self.capture_experience_impl(request).await
    }

    async fn list_benchmarks(
        &self,
        _request: Request<ListBenchmarksRequest>,
    ) -> Result<Response<ListBenchmarksResponse>, Status> {
        self.list_benchmarks_impl(_request).await
    }

    async fn get_breaker_metrics(
        &self,
        request: Request<BreakerMetricsRequest>,
    ) -> Result<Response<BreakerMetricsResponse>, Status> {
        self.get_breaker_metrics_impl(request).await
    }

    async fn get_learning_insights(
        &self,
        request: Request<sisyphus_bridge_api::LearningInsightsRequest>,
    ) -> Result<Response<sisyphus_bridge_api::LearningInsightsResponse>, Status> {
        self.get_learning_insights_impl(request).await
    }

    async fn start_chaos(
        &self,
        request: Request<sisyphus_bridge_api::ChaosStartRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStartResponse>, Status> {
        self.start_chaos_impl(request).await
    }

    async fn stop_chaos(
        &self,
        _request: Request<sisyphus_bridge_api::ChaosStopRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStopResponse>, Status> {
        self.stop_chaos_impl(_request).await
    }

    async fn get_chaos_status(
        &self,
        _request: Request<sisyphus_bridge_api::ChaosStatusRequest>,
    ) -> Result<Response<sisyphus_bridge_api::ChaosStatusResponse>, Status> {
        self.get_chaos_status_impl(_request).await
    }

    async fn stream_anomaly_report(
        &self,
        request: Request<sisyphus_bridge_api::AnomalyRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<sisyphus_bridge_api::AnomalyReport>>, Status>
    {
        self.stream_anomaly_report_impl(request).await
    }

    async fn stream_events(
        &self,
        _request: Request<sisyphus_bridge_api::EventRequest>,
    ) -> Result<Response<tonic::codegen::BoxStream<sisyphus_bridge_api::BridgeEvent>>, Status> {
        self.stream_events_impl(_request).await
    }

    // Experience sharing (federated learning)
    async fn share_experience(
        &self,
        request: Request<ShareExperienceRequest>,
    ) -> Result<Response<ShareExperienceResponse>, Status> {
        self.share_experience_impl(request).await
    }

    async fn pull_experience(
        &self,
        request: Request<PullExperienceRequest>,
    ) -> Result<Response<PullExperienceResponse>, Status> {
        self.pull_experience_impl(request).await
    }

    async fn list_shared_experiences(
        &self,
        request: Request<ListSharedExperiencesRequest>,
    ) -> Result<Response<ListSharedExperiencesResponse>, Status> {
        self.list_shared_experiences_impl(request).await
    }
}
