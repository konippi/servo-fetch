import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "test/**/*.test.ts"],
    testTimeout: 20_000,
    retry: process.env.CI ? 2 : 0,
  },
});
