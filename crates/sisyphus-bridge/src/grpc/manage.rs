//! gRPC 处理器（二）：管理域（代理、工具、技能、插件、工作区、会话）。

use crate::MySisyphusBridge;
use sisyphus_bridge_api::{
    AgentMetadata, CreateWorkspaceRequest, CreateWorkspaceResponse, DeleteWorkspaceRequest,
    DeleteWorkspaceResponse, GetCurrentWorkspaceRequest, GetCurrentWorkspaceResponse,
    InstallSkillRequest, InstallSkillResponse, ListAgentsRequest, ListAgentsResponse,
    ListPluginsRequest, ListPluginsResponse, ListSkillsRequest, ListSkillsResponse,
    ListSoPsRequest, ListSoPsResponse, ListToolsRequest, ListToolsResponse, ListWorkspacesRequest,
    ListWorkspacesResponse, LoadPluginRequest, LoadPluginResponse, PluginDescriptor,
    RemoveSkillRequest, RemoveSkillResponse, ShowSkillRequest, ShowSkillResponse, SkillMetadata,
    SopInfo, SwitchWorkspaceRequest, SwitchWorkspaceResponse, ToolMetadata, UnloadPluginRequest,
    UnloadPluginResponse, WorkspaceMetadata,
};
use tonic::{Request, Response, Status};

impl MySisyphusBridge {
    pub(crate) async fn list_agents_impl(
        &self,
        _request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let agents = self.agent_registry.list();
        Ok(Response::new(ListAgentsResponse {
            agents: agents
                .into_iter()
                .map(|(id, info)| {
                    let description = if !info.tool_prefixes.is_empty() {
                        let tools = self.tool_registry.list_for_prefixes(&info.tool_prefixes);
                        if tools.is_empty() {
                            info.description
                        } else {
                            let tool_names: Vec<&str> =
                                tools.iter().map(|(_, t)| t.name.as_str()).collect();
                            format!("{} — Tools: {}", info.description, tool_names.join(", "))
                        }
                    } else {
                        info.description
                    };
                    AgentMetadata {
                        agent_id: id,
                        display_name: info.display_name,
                        description,
                        tool_prefixes: info.tool_prefixes,
                        max_concurrency: info.max_concurrency as u32,
                    }
                })
                .collect(),
        }))
    }

    pub(crate) async fn list_tools_impl(
        &self,
        _request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        let tools = self.tool_registry.list();
        Ok(Response::new(ListToolsResponse {
            tools: tools
                .into_iter()
                .map(|(tool_id, info)| {
                    let source_str = match &info.source {
                        sisyphus_tools_mcp::tool_registry::ToolSource::Builtin => "builtin",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Mcp { .. } => "mcp",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Strategic => "strategic",
                        sisyphus_tools_mcp::tool_registry::ToolSource::Wasm => "wasm",
                    };
                    let params_json = info.parameters.map(|p| p.to_string()).unwrap_or_default();
                    ToolMetadata {
                        tool_id,
                        name: info.name,
                        description: info.description,
                        source: source_str.into(),
                        parameters_json: params_json,
                    }
                })
                .collect(),
        }))
    }

    pub(crate) async fn list_so_ps_impl(
        &self,
        _request: Request<ListSoPsRequest>,
    ) -> Result<Response<ListSoPsResponse>, Status> {
        let kernel = self.kernel.clone();
        let ids = kernel
            .execution_manager
            .list_sops(None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut sops = Vec::with_capacity(ids.len());
        for sop_id in ids {
            // node_count needs the graph; a failed load degrades to 0 rather
            // than dropping the SOP from the listing.
            let node_count = match kernel.execution_manager.load_sop(&sop_id, "default").await {
                Ok(graph) => graph.graph.node_count() as u32,
                Err(_) => 0,
            };
            sops.push(SopInfo {
                sop_id,
                created_at: 0,
                node_count,
            });
        }

        Ok(Response::new(ListSoPsResponse { sops }))
    }

    pub(crate) async fn list_skills_impl(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let req = request.into_inner();

        let skills = if req.remote {
            self.skill_registry
                .fetch_available_skills()
                .await
                .map_err(|e| Status::internal(format!("Remote registry error: {e}")))?
        } else {
            self.skill_registry
                .list_installed_skills()
                .await
                .map_err(|e| Status::internal(format!("Local store error: {e}")))?
        };

        let proto_skills: Vec<SkillMetadata> = skills
            .into_iter()
            .map(|s| SkillMetadata {
                id: s.id,
                name: s.name,
                description: s.description,
                version: s.version,
                author: s.author,
                tags: s.tags,
                created_at: s.created_at,
            })
            .collect();

        Ok(Response::new(ListSkillsResponse {
            skills: proto_skills,
        }))
    }

    pub(crate) async fn show_skill_impl(
        &self,
        request: Request<ShowSkillRequest>,
    ) -> Result<Response<ShowSkillResponse>, Status> {
        let req = request.into_inner();
        match self.skill_registry.get_skill(&req.name).await {
            Ok(Some(skill)) => Ok(Response::new(ShowSkillResponse {
                found: true,
                skill: Some(SkillMetadata {
                    id: skill.metadata.id,
                    name: skill.metadata.name,
                    description: skill.metadata.description,
                    version: skill.metadata.version,
                    author: skill.metadata.author,
                    tags: skill.metadata.tags,
                    created_at: skill.metadata.created_at,
                }),
            })),
            Ok(None) => Ok(Response::new(ShowSkillResponse {
                found: false,
                skill: None,
            })),
            Err(e) => Err(Status::internal(format!("Store error: {e}"))),
        }
    }

    pub(crate) async fn install_skill_impl(
        &self,
        request: Request<InstallSkillRequest>,
    ) -> Result<Response<InstallSkillResponse>, Status> {
        let req = request.into_inner();
        match self.skill_registry.install_skill(&req.skill_id).await {
            Ok(()) => Ok(Response::new(InstallSkillResponse {
                success: true,
                error: String::new(),
                name: req.skill_id.clone(),
            })),
            Err(e) => Ok(Response::new(InstallSkillResponse {
                success: false,
                error: e.to_string(),
                name: String::new(),
            })),
        }
    }

    pub(crate) async fn remove_skill_impl(
        &self,
        request: Request<RemoveSkillRequest>,
    ) -> Result<Response<RemoveSkillResponse>, Status> {
        let req = request.into_inner();
        match self.skill_registry.uninstall_skill(&req.name).await {
            Ok(()) => Ok(Response::new(RemoveSkillResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(RemoveSkillResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    pub(crate) async fn list_plugins_impl(
        &self,
        _request: Request<ListPluginsRequest>,
    ) -> Result<Response<ListPluginsResponse>, Status> {
        let plugins = self.plugin_manager.list_plugins();
        let descriptors: Vec<PluginDescriptor> = plugins
            .into_iter()
            .map(|p| PluginDescriptor {
                name: p.name,
                version: p.version,
                wasm_path: p.wasm_path.to_string_lossy().to_string(),
                tool_id: p.tool_id,
                fuel_limit: p.fuel_limit,
                checksum: p.checksum.unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListPluginsResponse {
            plugins: descriptors,
        }))
    }

    pub(crate) async fn load_plugin_impl(
        &self,
        request: Request<LoadPluginRequest>,
    ) -> Result<Response<LoadPluginResponse>, Status> {
        let req = request.into_inner();
        let descriptor = sisyphus_types::PluginDescriptor {
            name: req.name,
            version: req.version,
            wasm_path: std::path::PathBuf::from(&req.wasm_path),
            tool_id: req.tool_id,
            fuel_limit: if req.fuel_limit > 0 {
                req.fuel_limit
            } else {
                self.default_fuel
            },
            checksum: None,
        };
        match self.plugin_manager.register_plugin(descriptor).await {
            Ok(()) => Ok(Response::new(LoadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(LoadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }

    pub(crate) async fn unload_plugin_impl(
        &self,
        request: Request<UnloadPluginRequest>,
    ) -> Result<Response<UnloadPluginResponse>, Status> {
        let name = request.into_inner().name;
        match self.plugin_manager.unload_plugin(&name) {
            Ok(()) => Ok(Response::new(UnloadPluginResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(UnloadPluginResponse {
                success: false,
                error: e,
            })),
        }
    }

    pub(crate) async fn create_workspace_impl(
        &self,
        request: Request<CreateWorkspaceRequest>,
    ) -> Result<Response<CreateWorkspaceResponse>, Status> {
        let req = request.into_inner();
        match self.workspace_manager.create_workspace(req.name, req.project_id, req.root_dir).await
        {
            Ok(ws) => Ok(Response::new(CreateWorkspaceResponse {
                success: true,
                error: String::new(),
                workspace_id: ws.id,
            })),
            Err(e) => Ok(Response::new(CreateWorkspaceResponse {
                success: false,
                error: e.to_string(),
                workspace_id: String::new(),
            })),
        }
    }

    pub(crate) async fn switch_workspace_impl(
        &self,
        request: Request<SwitchWorkspaceRequest>,
    ) -> Result<Response<SwitchWorkspaceResponse>, Status> {
        let req = request.into_inner();
        match self.workspace_manager.switch_workspace(&req.workspace_id).await {
            Ok(()) => Ok(Response::new(SwitchWorkspaceResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(SwitchWorkspaceResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }

    pub(crate) async fn list_workspaces_impl(
        &self,
        _request: Request<ListWorkspacesRequest>,
    ) -> Result<Response<ListWorkspacesResponse>, Status> {
        let workspaces = self.workspace_manager.list_workspaces().await;
        let proto_ws: Vec<WorkspaceMetadata> = workspaces
            .into_iter()
            .map(|ws| WorkspaceMetadata {
                id: ws.id,
                name: ws.name,
                project_id: ws.project_id,
                root_dir: ws.root_dir,
            })
            .collect();
        Ok(Response::new(ListWorkspacesResponse {
            workspaces: proto_ws,
        }))
    }

    pub(crate) async fn get_current_workspace_impl(
        &self,
        _request: Request<GetCurrentWorkspaceRequest>,
    ) -> Result<Response<GetCurrentWorkspaceResponse>, Status> {
        match self.workspace_manager.get_current().await {
            Some(ws) => Ok(Response::new(GetCurrentWorkspaceResponse {
                found: true,
                workspace: Some(WorkspaceMetadata {
                    id: ws.id,
                    name: ws.name,
                    project_id: ws.project_id,
                    root_dir: ws.root_dir,
                }),
            })),
            None => Ok(Response::new(GetCurrentWorkspaceResponse {
                found: false,
                workspace: None,
            })),
        }
    }

    pub(crate) async fn delete_workspace_impl(
        &self,
        request: Request<DeleteWorkspaceRequest>,
    ) -> Result<Response<DeleteWorkspaceResponse>, Status> {
        let req = request.into_inner();
        match self.workspace_manager.delete_workspace(&req.workspace_id).await {
            Ok(()) => Ok(Response::new(DeleteWorkspaceResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(DeleteWorkspaceResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }
}
