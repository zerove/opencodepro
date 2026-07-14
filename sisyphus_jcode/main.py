import asyncio
from sisyphus_jcode.core.sop_graph import SOPGraph, SOPGraphConfig, SOPNodeConfig, NodeKind
from sisyphus_jcode.core.executor import JcodeExecutor
from sisyphus_jcode.core.cognitive_loop import CognitiveLoop, LlmClient
from sisyphus_jcode.core.synthesizer import SOPSynthesizer
from sisyphus_jcode.memory.golden_dataset import GoldenDataset

async def main():
    # 1. 初始化基础设施
    llm = LlmClient(agent_ref=None)
    executor = JcodeExecutor()
    golden_dataset = GoldenDataset()
    synthesizer = SOPSynthesizer(llm)
    loop = CognitiveLoop(executor, llm, golden_dataset)
    
    # 2. 定义目标
    goal = "Implement a high-performance cache and verify its correctness"
    tools = ["bash_exec", "file_edit", "lsp_search", "cargo_test"]
    
    print(f"🚀 Sisyphus-jcode 启动: 目标 = {goal}")
    
    # 3. 自动合成 SOP
    print("\n[1/3] 正在合成 SOP...")
    graph, plan_desc = await synthesizer.synthesize(goal, tools)
    print(f"合成成功! 计划描述: {plan_desc}")
    
    # 4. 进入认知执行循环
    print("\n[2/3] 进入认知执行循环...")
    result, output = await loop.run_sop(goal, graph)
    
    # 5. 处理最终结果
    print("\n[3/3] 最终分析:")
    if result == "SUCCESS":
        print("✅ 任务成功完成并已存入黄金数据集！")
    elif result == "PIVOT":
        print(f"⚠️ 触发战略转向 (PIVOT)! 原因: {output}")
        print("$\rightarrow$ 接下来将调用 Synthesizer 进行重新规划...")
    elif result == "REGRESSION":
        print("❌ 触发回归警报! 方案虽然通过但性能/稳定性低于黄金标准。")
    else:
        print(f"结果: {result}, 输出: {output}")

if __name__ == "__main__":
    asyncio.run(main())
