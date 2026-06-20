# OpenCodePro - Workspace Context

This workspace has been initialized as a fresh, empty repository for development. This file (`GEMINI.md`) serves as the foundational instruction context for Gemini CLI and other AI development assistants working in this project.

## Project Overview

*   **Status:** Newly initialized workspace.
*   **Purpose:** [TODO: Describe the primary purpose, features, and goals of this project]
*   **Target Stack:** [TODO: Specify primary technologies e.g., React/TypeScript, Rust, Python, Go]

As the project structure is created, please update this file with specific architectural decisions and tech stack details.

---

## Build & Run Commands

Once the tech stack and dependencies are configured, update this section with the exact commands.

### Build & Package
*   Build command: `[TODO: e.g., npm run build / cargo build]`
*   Production build: `[TODO: e.g., npm run build:prod / cargo build --release]`

### Run & Dev
*   Development server: `[TODO: e.g., npm run dev / cargo run]`
*   Production run: `[TODO: e.g., npm start]`

### Testing
*   Run unit tests: `[TODO: e.g., npm test / cargo test]`
*   Run specific test: `[TODO: e.g., npm test -- -t "test-name"]`
*   Test coverage: `[TODO: e.g., npm run coverage]`

### Formatting & Linting
*   Lint code: `[TODO: e.g., npm run lint / cargo clippy]`
*   Format code: `[TODO: e.g., npm run format / cargo fmt]`

---

## Development Conventions

As a general guideline, all code developed in this workspace should adhere to the following principles:

### 1. Code Quality & Architecture
*   **Type Safety:** Prefer strong typing (TypeScript, Rust, Go, Python with type hints) where applicable. Avoid `any` or disabling type-checker warnings.
*   **Explicit Composition:** Prioritize explicit composition and delegation over deep inheritance patterns.
*   **No Hidden Logic:** Avoid prototype manipulation, reflection hacks, or hidden state transitions. Keep data flows traceable and pure.

### 2. Testing Standards
*   **Test-First Approach:** Prefer Test-Driven Development (TDD) for core logic and business rules.
*   **Verification:** Always add automated tests for new features, bug fixes, or structural changes. Validate locally before pushing or marking work complete.
*   **Integration Tests:** Write end-to-end or integration tests to verify the boundaries between major modules.

### 3. Version Control (Conventional Commits)
When staging and committing changes, use Conventional Commits format for clarity:
*   `feat(...)`: New feature
*   `fix(...)`: Bug fix
*   `docs(...)`: Documentation changes
*   `style(...)`: Formatting/styling (no logic change)
*   `refactor(...)`: Code change that neither fixes a bug nor adds a feature
*   `test(...)`: Adding missing tests or correcting existing tests
*   `chore(...)`: Changes to build process, auxiliary tools, or libraries

### 4. Token-Optimized Development
*   **RTK Integration:** This workspace is optimized using the **Rust Token Killer (rtk)** CLI proxy.
*   Always use `rtk` commands directly where applicable to optimize token consumption during development tasks.

---

## Workspace Directory Structure

*Below is the starting layout. As directories are created, describe their roles here.*

```
/home/zeron/dev/opencodepro/
└── GEMINI.md            # This instruction context file
```
