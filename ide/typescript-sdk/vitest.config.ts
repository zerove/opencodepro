// vitest 配置文件，用于 SDK 单元测试
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // 使用 node 环境（无需 jsdom，因为 SDK 只用 fetch）
    environment: 'node',
    // 测试文件匹配模式
    include: ['src/__tests__/**/*.test.ts'],
    // 每个测试文件超时 10 秒
    testTimeout: 10_000,
  },
});
