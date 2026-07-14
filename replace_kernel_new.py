import sys

def replace_new_method(file_path):
    with open(file_path, 'r') as f:
        lines = f.readlines()

    new_method = r'''    pub fn new(
        state_store: Arc<MfsStore>,
        sop_store: Arc<SopSqlStore>,
        vector_store: Arc<sisyphus_mem::VectorMemoryStore>,
        experience_bank: Arc<dyn sisyphus_types::experience::ExperienceBank>,
        tool_executor: Arc<dyn ToolExecutor>,
        intel: Arc<SisyphusIntel>,
        llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        fast_llm_client: Arc<dyn sisyphus_tools_llm::LlmClient>,
        synthesizer: Arc<dyn sisyphus_intel::FlowSynthesizer>,
        meta_learner: Arc<parking_lot::Mutex<MetaLearner>>,
        consensus: Arc<dyn Consensus>,
        sql_store: Arc<sisyphus_mem::RoadmapSqlStore>,
        embedding_client: Arc<dyn sisyphus_tools_llm::EmbeddingClient>,
    ) -> Self {
        let pm = Arc::new(sisyphus_roadmap::proposal_manager::ProposalManager::new(state_store.clone()));
        let rm = Arc::new(RoadmapManager::new(state_store.clone(), sql_store, pm.clone()));
        
        let test_executor = Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new());
        
        let intelligence_manager = Arc::new(IntelligenceManager::new(
            Arc::new(SisyphusIntel::new(
                intel.simulator.clone(),
                Arc::new(sisyphus_intel::simulation::LlmLogicalSimulator::new(llm_client.clone())),
                intel.reflector.clone(),
                intel.evolver.clone(),
            )),
            synthesizer,
            llm_client,
        ));

        let knowledge_manager = Arc::new(sisyphus_knowledge::AssetManager::new(
            std::path::PathBuf::from(".sisyphus/knowledge"),
        ));

        let intervention_manager = Arc::new(crate::intervention_manager::InterventionManager::new());
        let internalization_manager = Arc::new(crate::internalization_manager::InternalizationManager::new(
            intelligence_manager.llm_client.clone(),
            knowledge_manager.clone(),
        ));
        let visual_manager = Arc::new(sisyphus_visual::VisualPerceptionManager::new(
            Arc::new(sisyphus_visual::SisyphusVisualProvider::new(intelligence_manager.llm_client.clone())),
            intelligence_manager.llm_client.clone(),
        ));

        let execution_manager = Arc::new(ExecutionManager::new(
            state_store,
            sop_store,
            tool_executor,
            test_executor.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
        ));

        let channel = Arc::new(AgentChannel::new());
        let benchmark = Arc::new(parking_lot::Mutex::new(ContinuousBenchmark::new(
            ".omo/benchmarks",
        )));
        
        let strategic_manager = Arc::new(strategic_manager::StrategicManager::new(
            intelligence_manager.clone(),
            execution_manager.clone(),
            sisyphus_types::config::CognitiveLoopConfig::default(),
            channel.clone(),
            benchmark.clone(),
            meta_learner.clone(),
            experience_bank.clone(),
            knowledge_manager.clone(),
            rm.clone(),
            intervention_manager.clone(),
            internalization_manager.clone(),
            Some(visual_manager.clone()),
        ));

        let micro_reflector = Arc::new(sisyphus_intel::micro_reflector::LlmMicroReflector::new(fast_llm_client));

        Self {
            execution_manager,
            intelligence_manager,
            strategic_manager,
            consensus,
            intervention_manager,
            channel,
            benchmark,
            meta_learner,
            roadmap_manager: rm,
            proposal_manager: pm,
            experience_bank,
            micro_reflector: Some(micro_reflector),
            test_executor,
            strategic_reflector: None,
            milestone_generator: None,
            resource_optimizer: sisyphus_roadmap::resource_optimizer::ResourceOptimizer::new(),
        }
    }
'''

    start_idx = -1
    for i, line in enumerate(lines):
        if line.startswith('    pub fn new('):
            start_idx = i
            break
    
    if start_idx == -1:
        print("Could not find new() method")
        return

    # Find where the function body actually starts
    body_start_idx = -1
    for i in range(start_idx, len(lines)):
        if '{' in lines[i]:
            body_start_idx = i
            break
            
    if body_start_idx == -1:
        print("Could not find opening brace for new()")
        return

    # Find the matching closing brace for the body
    brace_count = 0
    end_idx = -1
    for i in range(body_start_idx, len(lines)):
        brace_count += lines[i].count('{')
        brace_count -= lines[i].count('}')
        if brace_count == 0:
            end_idx = i
            break
            
    if end_idx == -1:
        print("Could not find closing brace for new()")
        return

    new_lines = lines[:start_idx] + [new_method + '\n'] + lines[end_idx+1:]
    
    with open(file_path, 'w') as f:
        f.writelines(new_lines)
    print(f"Successfully replaced new() method in {file_path}")

if __name__ == "__main__":
    replace_new_method(sys.argv[1])
