# Original prompt (verbatim)

This is the master directive the user gave at the start of the project. It defines the ultimate goal.

---

# FORGE — The Agentic Engineering Operating System

## MASTER BUILD DIRECTIVE

You are Claude Sonnet 5.5 operating at HIGH reasoning effort.

Act as the founding CTO, principal systems architect, elite product designer, security engineer, and lead developer of FORGE.

You have been entrusted with building an ambitious, genuinely useful software product from scratch.

**FORGE is an agentic engineering operating system that transforms existing AI coding agents into a coherent, observable, knowledge-preserving, continuously improving software engineering environment.**

It must be useful to a solo developer every single day, while establishing credible foundations for professional teams and eventual enterprise adoption.

This must not become another generic agent chat interface, IDE clone, token dashboard, or visual wrapper around terminal sessions.

### The core proposition

Existing coding agents are powerful but fragmented.

Developers must manually coordinate sessions, manage project context, preserve useful knowledge, configure hooks and skills, review diffs, track worktrees, run tests, investigate failures, and determine whether an agent's work is actually trustworthy.

These activities are spread across terminals, repositories, documentation, project trackers, and disconnected provider-specific tooling.

FORGE unifies the engineering process without replacing the underlying agents.

**Our product should make existing agents materially more effective, reliable, and useful.**

Every feature must answer at least one question:

1. Does this reduce repeated developer work?
2. Does it improve agent correctness or reliability?
3. Does it preserve useful knowledge across sessions?
4. Does it make complex development easier to understand?
5. Does it improve control, verification, or security?
6. Does it enable a workflow that existing coding tools handle poorly?

Features that do none of these should not be implemented.

---

# PART I — PRODUCT PHILOSOPHY

## 1. The Karpathy principles

Study these primary sources before architecture decisions:

- Andrej Karpathy's April 2026 LLM Wiki:
  https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f
- AutoResearch:
  https://github.com/karpathy/autoresearch
- Sequoia Ascent 2026:
  https://karpathy.bearblog.dev/sequoia-ascent-2026/
- OpenAI Harness Engineering:
  https://openai.com/index/harness-engineering/
- Anthropic's Claude Code customization guidance:
  https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- Codex as a platform:
  https://developers.openai.com/blog/codex-as-a-platform

Extract the actionable engineering principles, rather than implementing superficial imitations.

### A. Persistent knowledge compounds

Do not make agents rediscover the same architectural decisions, known problems, and project conventions on every session.

### B. Verification enables useful autonomy

Agents can perform increasingly difficult work if their outputs are systematically tested and evaluated.

### C. The engineering environment matters

Agent performance depends not only on the model but also on context quality, tools, instructions, permissions, hooks, and feedback.

### D. Human understanding remains essential

Provide interfaces that explain what the agents are doing, why decisions were made, and whether outputs are reliable.

### E. Avoid reinventing mature harnesses

Reuse existing agent runtimes. FORGE owns orchestration, project intelligence, memory, policies, evidence, and user experience.

Do not recreate Claude Code or Codex from scratch.

---

# PART II — USER EXPERIENCE

## 2. The daily developer workflow

Imagine a developer opening FORGE each morning.

They should immediately see:

- Their active projects
- Running and suspended agents
- Tasks awaiting human decisions
- Work completed since their last visit
- Failed builds and tests
- Changes ready for review
- Important architectural discoveries
- Newly identified risks
- Blocked tasks
- Resource and subscription usage where observable

The developer should be able to resume useful work with very little friction.

### Example interaction

The developer opens FORGE and selects a project.

They write:

"Implement user authentication, including password reset, proper session management, and tests."

FORGE should:

1. Inspect the project's structure and available documentation.
2. Retrieve relevant project knowledge.
3. Identify applicable coding policies.
4. Construct a task contract with acceptance criteria.
5. Let the user select an available coding agent.
6. Create an isolated worktree.
7. Launch and supervise the agent using a supported integration.
8. Present actual agent activity.
9. Collect changes, test results, and verification evidence.
10. Surface failures and request human intervention when necessary.
11. Produce an understandable review package.
12. Offer a controlled merge workflow.
13. Preserve confirmed engineering knowledge for future work.

This workflow is the foundation of the product.

The user should not need to navigate five separate tools to accomplish it.

---

# PART III — CORE PRODUCT SYSTEMS

## 3. MISSION CONTROL — Unified Agent Orchestration

Build a powerful central workspace.

### Core capabilities

- Register multiple local Git repositories.
- Display multiple projects simultaneously.
- Create engineering tasks and missions.
- Launch supported AI coding agents.
- Track sessions and their lifecycle.
- Resume sessions where provider capabilities permit.
- Cancel tasks and clean up processes safely.
- Inspect live output.
- Review command execution and file modifications.
- Handle permission and approval requests.
- Show agent status and failures.
- Track associated Git branches and worktrees.
- Associate tasks with requirements and verification results.

### Provider adapters

Design a provider-neutral interface.

Initial targets:

**Claude Code**
- Investigate officially supported programmatic integration surfaces.
- Prefer supported Claude Agent SDK or documented CLI interfaces where legally and technically appropriate.
- Preserve native permission handling and session capabilities.
- Detect installed CLI versions and supported capabilities.

**OpenAI Codex**
- Prefer the documented Codex app-server for rich interactive sessions.
- Use official Codex SDK or `codex exec` for bounded operations.
- Support structured events, streamed progress, session state, and approval flows where available.

Additional future adapters:
- OpenCode
- Gemini CLI
- Other agents implementing suitable standardized protocols

Investigate the Agent Client Protocol (ACP) and other relevant standards, but do not introduce them without a concrete benefit.

### Important integration constraints

Do not parse decorative terminal output as a substitute for structured events when a supported interface exists.

Do not assume different providers expose identical capabilities.

Maintain an explicit capability matrix covering:
- Session start/resume
- Streaming
- Interrupt
- Tool events
- File changes
- Approval handling
- Usage reporting
- Model selection
- Worktree integration

Unsupported capabilities must appear as unavailable, not as fake working controls.

### Agent workspace

Each active mission should have:

- Primary agent interaction panel
- Streaming activity timeline
- Current task checklist
- Worktree status
- Changed files
- Verification results
- Relevant memory
- Permission requests
- Human intervention controls

Let users inspect details without overwhelming them.

---

## 4. MISSION CONTRACTS — Define Success Before Coding

This is a primary differentiator.

Instead of launching vague prompts, FORGE should support structured engineering contracts.

A mission contract contains:

- Objective
- Repository and branch
- Relevant context
- In-scope files or modules
- Out-of-scope boundaries
- Acceptance criteria
- Required tests
- Security constraints
- Review expectations
- Resource limits
- Required approvals

The system should help users convert a loose request into concrete criteria.

Do not require extensive forms for small tasks.

Support lightweight missions that only need an objective, plus stronger contracts for sensitive or complex work.

### Example

Mission: Implement password reset.

Acceptance criteria:
- Secure reset-token generation
- Token expiration
- Correct account identity handling
- Prevention of account enumeration
- Relevant automated tests
- No unrelated modifications

At completion, FORGE must evaluate each criterion using available evidence.

Do not confuse an agent claiming completion with actual verification.

---

## 5. FORGE MEMORY — Persistent Engineering Intelligence

**This is one of the most important and distinctive systems.**

Build a durable memory engine inspired by Karpathy's LLM Wiki concept.

The goal is not merely storing conversation summaries.

The goal is accumulating a useful, structured understanding of software projects.

### Memory layers

**Layer 1 — Raw evidence**

Store references to original sources:

- Repository documentation
- Git commits
- Agent outputs
- Task contracts
- Test failures
- Build logs
- Approved decisions
- Incident reports
- Relevant user-provided documents

Preserve source identity, timestamps, and hashes when appropriate.

Do not silently modify original sources.

**Layer 2 — Project wiki**

Create interlinked Markdown knowledge pages:

- Project architecture
- Components
- Dependencies
- Design decisions
- Important interfaces
- Coding conventions
- Historical incidents
- Known failure modes
- Accepted solutions
- Technical debt
- Important experiments

**Layer 3 — Executable engineering knowledge**

Certain confirmed lessons can be proposed as:

- Agent instructions
- Skills
- Rules
- Test requirements
- Hook configurations
- Review checks
- Project templates

This distinction is critical.

A memory containing "Never use email addresses as persistent payment identity keys" might become a proposed authentication review rule.

But generated memories must not automatically gain the authority to execute commands or change security policy.

### Memory lifecycle

Implement:

**Capture → Extract → Verify → Link → Store → Retrieve → Refresh → Retire**

Knowledge must have:

- Source references
- Creation/update timestamps
- Scope
- Confidence or verification status
- Ownership
- Related files/components
- Supersession relationships where relevant

Support states such as:
- Proposed
- Verified
- Superseded
- Disputed
- Archived

Do not promote agent-generated speculation into authoritative project rules automatically.

### Memory retrieval

At mission start:

1. Identify the files, modules, and concepts involved.
2. Search relevant project knowledge.
3. Retrieve only useful material.
4. Rank knowledge by relevance, reliability, recency, and scope.
5. Construct a compact, source-backed context package.
6. Let users inspect what was injected.

Do not dump the entire knowledge base into every agent context window.

Start with Markdown indexing and SQLite full-text search.

Use embeddings only if they demonstrate measurable retrieval improvement on realistic project queries.

### Memory quality controls

Implement:
- Duplicate detection
- Contradiction detection
- Staleness warnings
- Broken references
- Orphaned concepts
- Source verification
- Manual promotion/rejection
- Project-specific versus global memory separation

The system should become more useful over months of work without growing uncontrollably.

---

## 6. OBSIDIAN — First-Class Knowledge Integration

Obsidian integration must be genuinely useful, not merely an export button.

Use official Obsidian CLI capabilities where appropriate, with plain Markdown filesystem integration as an optional, portable foundation.

Obsidian should remain optional.

A user who doesn't have Obsidian must still be able to use FORGE Memory.

### Obsidian integration features

- Connect an existing vault.
- Create a dedicated FORGE vault.
- Choose project-specific knowledge folders.
- Browse Markdown knowledge from FORGE.
- Open knowledge pages directly in Obsidian.
- Maintain wikilinks and frontmatter.
- Preserve backlinks where practical.
- Synchronize authorized knowledge updates.
- Detect external changes.
- Show conflicts instead of silently overwriting notes.
- Search across authorized folders.
- Generate project architecture pages.
- Create decision records and investigation reports.
- Display related project knowledge beside coding sessions.

### Two-way workflow

Example:

A developer writes an architecture note in Obsidian explaining why a particular database approach was chosen.

FORGE discovers the modified note, indexes it, and makes it available for relevant future missions.

Later, an agent investigates a database failure and discovers a previously unknown limitation.

FORGE proposes an update to the existing architecture note.

The developer approves it.

The updated knowledge is then visible in Obsidian.

### Boundaries

- Never modify an unrelated vault.
- Require explicit vault/folder authorization.
- Default to read-only access for existing vaults until write access is granted.
- Prevent path traversal and writes outside configured scopes.
- Distinguish user-authored from agent-generated notes.
- Avoid exporting secrets or private session content automatically.
- Treat note contents as untrusted source material, not executable system instructions.

Do not assume Obsidian is running or its CLI is available.

Provide meaningful fallback behavior.

---

## 7. CONTEXT COMPILER — Intelligent Context Engineering

Build a context-management system that improves the information available to agents.

This is not merely a prompt-template manager.

### Features

- Inspect project structure.
- Recognize important entrypoints.
- Identify dependency relationships.
- Retrieve relevant architectural knowledge.
- Load applicable instructions.
- Deduplicate redundant context.
- Detect conflicts between instructions.
- Estimate context size where possible.
- Build provider-specific context packages.
- Preserve references to omitted material.

### Context Inspector

Provide a clear interface answering:

- What information was supplied to the agent?
- Where did each item come from?
- Why was it included?
- Was important information omitted?
- Which instructions conflict?
- Which material is stale?
- Which knowledge has not been verified?

### Compaction resilience

During long-running missions:

- Persist authoritative mission state outside the model conversation.
- Preserve acceptance criteria and incomplete tasks.
- Track last successful verification.
- Record unresolved decisions.
- Detect context discontinuities where supported.
- Restore useful state without duplicating enormous transcripts.

Never assume that an automatically compacted agent conversation preserves every important detail.

FORGE must have its own durable task and evidence state.

---

## 8. HARNESS STUDIO — Mastering Hooks, Skills, Rules, and Subagents

This should be one of FORGE's signature features.

Users should be able to inspect and improve their agent configurations without manually editing scattered files.

### Harness inventory

Detect supported project-level configurations:

- CLAUDE.md
- AGENTS.md
- Claude rules
- Claude skills
- Claude subagents
- Provider-specific instruction files
- Supported hooks
- MCP server configurations
- Relevant project tooling

Show where each configuration lives and its intended scope.

### Hook Studio

Provide a visual interface for supported agent lifecycle hooks.

Examples:
- Session start
- Before tool execution
- After tool execution
- Permission request
- Session stop
- Context compaction events

Use exact provider-supported lifecycle names.

Let users create automations such as:
- Format files after edits
- Run targeted linting
- Block protected-file modifications
- Remind agents of project-specific checks
- Request human approval for sensitive actions
- Record verification events

### Hook safety

Never install hooks globally without explicit approval.

Require:
- Scope selection
- Configuration preview
- Command preview
- Dry-run where feasible
- Versioned backups
- Rollback
- Clear provenance
- Timeout and failure behavior

Treat hooks as potentially privileged code execution.

Provider-native permissions must remain authoritative.

FORGE must not silently bypass or override a provider's safety controls.

### Skills Workshop

Users should be able to:

- Discover existing skills
- Inspect their instructions
- Understand activation conditions
- Find redundant skills
- Create new focused skills
- Edit and version skills
- Test skills against example tasks
- Export supported formats

Favor small, targeted skills rather than enormous universal prompts.

### Subagent configuration

Support reviewing and editing appropriate subagent definitions.

Explain the distinction between:
- Procedural skills
- Isolated specialized subagents
- Persistent project rules
- Deterministic hooks

Do not treat these mechanisms as interchangeable.

### Harness Doctor

Analyze configurations and flag:
- Contradictory instructions
- Excessive always-loaded context
- Duplicate skills
- Ineffective or broken hooks
- Dangerous commands
- Unnecessary tool exposure
- Stale paths
- Provider compatibility problems

Provide actionable explanations and previews before fixes.

---

## 9. PROOF ENGINE — Evidence-Based Agent Verification

**This is the other flagship differentiator alongside Memory.**

Every substantial mission should generate a Proof Pack.

A Proof Pack is a reviewable record of what the agent actually accomplished.

It contains:

- Original task contract
- Relevant context references
- Agent/session identity
- Worktree and commit identity
- Files changed
- Git diff
- Build results
- Test outcomes
- Static analysis results
- Security checks performed
- Acceptance-criteria status
- Known unresolved issues
- Human approvals
- Final disposition

### Evidence graph

Build a visual connection between:

**Requirement → Changed code → Verification → Outcome**

Example:

Requirement: Reset tokens expire.

Evidence:
- Relevant code change
- Added expiration test
- Test execution result
- Commit reference

Status: Verified by passing automated test.

Another requirement might be marked:

Status: Not yet verified.

Avoid a fake "AI confidence score" or invented correctness percentages.

### Verification states

Use precise language:
- Not checked
- Check pending
- Verified by specified test
- Failed
- Inconclusive
- Requires manual review

Passing tests is evidence, not an absolute guarantee of correctness.

### Review experience

Include:
- Side-by-side diffs
- Changed-file navigation
- Test output
- Inline review comments
- Acceptance-criteria checklist
- Approval/rejection
- Request changes
- Rerun failed checks
- Open changes in external editor

A developer should be able to determine whether work is ready to merge without reading thousands of lines of terminal history.

---

## 10. AUTORESEARCH LAB — Controlled Engineering Experiments

Inspired by Karpathy's AutoResearch.

Build an experimental subsystem that lets developers optimize code or workflows against measurable criteria.

### Example

Mission: Reduce database query latency.

Baseline:
- Benchmark median: 180 ms

Agent proposes a change.

FORGE runs a defined benchmark.

New result:
- Benchmark median: 132 ms

FORGE records:
- Exact code revision
- Benchmark command
- Environment details
- Result
- Measurement uncertainty
- Whether correctness checks passed

The user decides whether to retain the change.

### Other experiments

- Improve test execution time
- Reduce binary size
- Reduce memory usage
- Compare refactoring strategies
- Evaluate alternative libraries
- Test different context retrieval strategies
- Compare skill configurations
- Compare agent prompting strategies

### Experimental integrity

- Isolate experimental modifications.
- Capture baseline measurements.
- Use repeatable evaluation commands.
- Apply warmup and repetition where necessary.
- Preserve failed and inconclusive outcomes.
- Require correctness tests before accepting performance gains.
- Avoid changing multiple uncontrolled variables simultaneously.
- Allow rollback.
- Never optimize arbitrary proxy metrics at the expense of specified requirements.

### Harness experiments

Eventually, users should be able to evaluate whether a modified skill, hook, or context policy actually improves agent performance.

This creates an evidence-based way to improve the engineering harness.

**Do not claim continuous self-improvement unless outcomes are actually measured.**

Start with explicit user-triggered experiments, not unlimited autonomous loops.

---

# PART IV — WHAT MAKES FORGE A DAILY-USE PRODUCT

## 11. Engineering Inbox

Build a unified work queue showing:

- Missions ready to start
- Active work
- Blocked agents
- Permission requests
- Completed tasks awaiting review
- Failed checks
- Merge conflicts
- Work needing clarification

Prioritize actual actions rather than decorative productivity scores.

Support keyboard-first navigation and a command palette.

## 12. Git Worktree Manager

Support:
- Create worktree for a mission
- View associated branch
- Inspect dirty state
- Show changes and commits
- Detect conflicting work
- Open worktree in editor
- Safely archive or remove completed worktrees
- Merge changes with proper checks and approvals

Protect uncommitted work.

Never delete a worktree containing unsaved or uncommitted changes without an explicit confirmation that identifies the consequences.

## 13. Agent Flight Recorder

Record structured agent activity so users can investigate what happened.

Events include:
- Mission creation
- Agent launch
- Tool execution
- File edits
- Permission decisions
- Test execution
- Failures
- Review checkpoints
- Cancellation
- Completion

Provide filtering and timeline inspection.

Support recovery after application crashes where provider and process state permit.

Do not pretend a detached or lost session is still running.

Keep sensitive information out of logs wherever possible.

## 14. Failure Intelligence

When a mission fails:

1. Preserve the error and relevant evidence.
2. Determine whether the failure is deterministic.
3. Associate it with previous similar failures.
4. Retrieve known solutions when available.
5. Suggest a repair strategy.
6. Offer an explicitly authorized retry.
7. Record what resolved the issue.

Distinguish:
- Build errors
- Test failures
- Permission problems
- Agent crashes
- Rate limits
- Integration failures
- Conflicting worktree changes
- Environment problems

Avoid uncontrolled retry loops.

## 15. Resource and Usage Awareness

Display reliable provider-reported usage information when available.

Include:
- Session duration
- Tool activity
- Reported token usage
- Reported cost or usage allowance, where supported
- Active process count
- Mission elapsed time
- Historical execution time

Clearly distinguish real measurements from estimates.

Allow concurrency limits.

Never claim exact subscription usage when the provider does not expose it.

---

# PART V — ENTERPRISE FOUNDATIONS

## 16. Design for Teams Without Overbuilding V1

FORGE must be architected to eventually support organizations.

However, the first release should not pretend to be enterprise-ready merely because it has a login page and a Team Settings tab.

### Future-compatible capabilities

Design interfaces and schemas for:

- Organizations
- Workspaces
- Users
- Roles
- Project permissions
- Approval policies
- Team knowledge
- Audit records
- Provider credentials
- Shared configurations
- Compliance policies
- Remote agent environments
- GitHub/GitLab integration
- CI results
- Organizational reports

### Enterprise requirements

A serious enterprise version will need:

- SSO/OIDC
- RBAC
- Centralized policy management
- Organization-scoped audit logs
- Protected branches
- Review/merge authorization
- Secrets-management integration
- Data retention controls
- Per-project access boundaries
- Tenant isolation
- Remote execution isolation
- Administrator controls
- Observability
- Disaster recovery

Design clear boundaries now, but implement only what can be responsibly tested.

Do not falsely advertise SOC 2 compliance, certifications, or enterprise security guarantees.

### Security model

Assume repositories, MCP outputs, documents, terminal output, and agent-generated text may contain untrusted instructions.

Keep data separate from authority.

Protect:
- Credentials
- Environment variables
- Private source code
- User vaults
- Files outside the workspace
- Git history
- Remote services
- Local shell execution

Sensitive actions require enforceable permissions.

Approvals should be narrowly scoped, attributable, and recorded.

Prevent an agent from approving its own restricted actions through a separate tool path.

A UI confirmation is insufficient if the backend can be bypassed.

### Provider authentication

Use officially supported and permitted authentication methods.

For personal local development, support existing authenticated local tools where allowed.

For a distributed commercial or enterprise product, use provider-approved integration and billing flows.

Never capture, copy, or redistribute provider OAuth/session tokens from another application.

Do not assume personal subscription authentication grants unlimited commercial embedding rights.

Do not silently switch to separately billed API usage.

---

# PART VI — DESIGN SYSTEM

## 17. Visual Direction

Build a premium, information-dense desktop application.

Not a browser SaaS template.

Not a generic purple AI dashboard.

Not a collection of cards with random gradients.

### Design language

- Near-black and charcoal surfaces
- Neutral typography
- Muted cool accents
- Purposeful status colors
- Strong hierarchy
- Thin, elegant separators
- Dense but readable tables
- Excellent monospace typography
- Subtle animations
- Fast transitions
- Resizable panels
- Thoughtful empty states
- Excellent keyboard navigation
- Accessible controls
- Professional code-diff presentation

The product should feel like a modern developer workstation.

Think of the refinement of Linear, the navigational efficiency of professional IDEs, and the clarity of well-designed observability tools.

Do not copy their branding or interfaces.

### Primary navigation

1. Command Center
2. Projects
3. Missions
4. Agents
5. Knowledge
6. Harness Studio
7. Experiments
8. Settings

### Project workspace

Within an individual project, use a flexible multi-panel layout:

- Main mission content
- Agent execution feed
- Git changes
- Task contract
- Proof/verification results
- Context and knowledge inspector

Users must be able to expand the editor, terminal, or diff view when needed.

Use keyboard shortcuts and quick actions extensively.

### Interaction quality

Every clickable element must perform a real action.

Every status must correspond to real data.

No fake agent animations.

No simulated terminal activity in production.

No decorative graphs presenting invented values.

Prefer exceptional implementation of a few essential screens over shallow implementation of twenty screens.

---

# PART VII — TECHNICAL ARCHITECTURE

## 18. Technology Selection

Target a Windows-first local desktop application with portability in mind.

Preferred stack:

**Desktop**
- Tauri 2

**Frontend**
- React
- TypeScript
- Tailwind CSS
- Appropriate accessible component primitives
- A suitable code diff/editor component
- A restrained chart/visualization library

**Backend**
- Rust for native desktop functions, process supervision, storage, and privileged boundaries
- A separate TypeScript/Node runtime service only where provider SDKs, PTYs, or integrations make it justified

**Persistence**
- SQLite
- SQLite FTS5 for textual retrieval
- Markdown files for portable engineering knowledge
- Git for source-control history

**Integrations**
- Official Codex app-server/SDK where suitable
- Supported Claude Code/Agent SDK integration
- Obsidian CLI
- Git CLI or established Git libraries
- MCP when it materially helps
- CI providers in later phases

Before finalizing the stack, verify documentation and licensing.

### Core architecture

Create cleanly separated domains:

- Desktop UI
- Application command service
- Project repository service
- Provider adapter layer
- Session supervisor
- Mission orchestration
- Memory/indexing service
- Context compiler
- Harness configuration service
- Verification engine
- Experiment runner
- Policy/approval engine
- Persistence and audit service

Do not build a gigantic monolithic component or God service.

### Suggested data entities

- Project
- Mission
- MissionContract
- AgentSession
- ProviderCapability
- Worktree
- ActivityEvent
- ApprovalRequest
- VerificationRun
- VerificationEvidence
- ProofPack
- KnowledgeSource
- KnowledgePage
- KnowledgeAssertion
- MemoryLink
- HarnessConfiguration
- HookDefinition
- Experiment
- ExperimentResult
- AuditEvent

Specify relationships, identifiers, and state transitions before implementing persistence.

### Event architecture

Use typed domain events.

Persist important state transitions before declaring success.

Support event sequence numbers or equivalent ordering safeguards.

Account for:
- Duplicated events
- Out-of-order events
- Process restarts
- Crashes
- Partial work
- Provider timeouts
- Failed persistence operations

Do not require a distributed message broker for the local MVP.

### Windows considerations

- Correct subprocess cleanup
- Process-tree termination
- Windows path normalization
- UTF-8/Unicode paths
- Git worktree behavior on NTFS
- Locked files
- File watchers
- Terminal compatibility
- WSL support where appropriate
- Secure storage of configuration/secrets

Do not assume the developer runs macOS or Linux.

---

# PART VIII — DEVELOPMENT EXECUTION

## 19. Build an Actual Product, Not a Specification

Work autonomously and implement code.

Do not stop after writing an architecture document.

However, avoid the common failure of attempting every feature at once and producing shallow, nonfunctional implementations.

Use phased delivery with mandatory verification gates.

### Phase 0 — Architecture and Research

Inspect the environment.

Verify:
- Tauri toolchain
- Node/Rust availability
- Git
- Claude Code integration capabilities
- Codex app-server capabilities
- Obsidian CLI behavior
- Relevant provider terms and supported auth paths

Write:
- PRODUCT.md
- ARCHITECTURE.md
- DATA_MODEL.md
- SECURITY.md
- ROADMAP.md
- PROGRESS.md

Keep documents useful and concise.

### Phase 1 — Working Daily Driver (P0)

Implement:
- Native application shell
- Project registration
- Project switching
- Git status
- Mission creation
- One fully working provider adapter
- Real agent streaming
- Agent cancellation
- Approval handling
- Basic worktree isolation
- Persistent session/mission history
- Real task activity feed

**Gate:** A user can select a project, run an actual coding mission, see what happened, and return to it later.

Do not proceed until this is functional.

### Phase 2 — Real Engineering Verification (P0)

Implement:
- Mission contracts
- Git diff review
- Test execution
- Verification evidence
- Proof Packs
- Review approval/rejection
- Safe merge workflow

**Gate:** The user can run a coding task and determine whether its changes satisfy explicit acceptance criteria.

### Phase 3 — Memory and Obsidian (P0)

Implement:
- Knowledge source ingestion
- Markdown project wiki
- Project knowledge search
- Source references
- Memory retrieval for missions
- Memory inspection
- Obsidian vault integration
- Staleness and conflict handling

**Gate:** A verified discovery from one mission can be retrieved and used in another mission, with clear provenance.

### Phase 4 — Harness Studio (P1)

Implement:
- Configuration inventory
- CLAUDE.md/AGENTS.md inspection
- Skills browsing
- Supported hook inspection
- Safe configuration editing
- Hook templates
- Preview, backup, and rollback
- Harness diagnostics

**Gate:** A developer can inspect and safely change an actual hook or project instruction through FORGE and verify the resulting behavior.

### Phase 5 — Multi-Agent Capability (P1)

Implement:
- Second provider adapter
- Parallel missions in separate worktrees
- Concurrency controls
- Shared project visibility
- Conflict detection
- Independent review workflows

**Gate:** Claude Code and Codex can be used on independent tasks without corrupting the same workspace.

### Phase 6 — Experiment Engine (P2)

Implement:
- Baseline measurement
- Controlled code experiment
- Comparison of results
- Result history
- Keep/discard workflow
- Experiment evidence export

**Gate:** FORGE can run a real engineering optimization experiment and show reproducible evidence of improvement or regression.

### Phase 7 — Enterprise Extensions (Future)

Only after the local product is stable:

- Team services
- Shared memory
- Organizational policies
- Remote runners
- SSO
- Centralized approvals
- CI integration
- Audit export

Keep this separate from the first release unless the infrastructure naturally supports it.

---

# PART IX — DEVELOPMENT QUALITY

## 20. Testing Standards

Implement automated tests at several layers.

### Unit tests
- Mission state transitions
- Memory retrieval/ranking
- Context selection
- Capability negotiation
- Configuration validation
- Approval policy
- Path security
- Experiment-result comparisons

### Integration tests
- Git worktree creation and cleanup
- Provider event parsing
- Session lifecycle
- SQLite persistence
- Obsidian read/write against a test vault
- Hook configuration roundtrip
- Proof Pack generation

### Failure scenarios
- Agent process crashes
- Application restarts during active work
- Provider unavailable
- Subscription usage exhausted
- Malformed provider events
- Concurrent writes
- Missing Git installation
- Merge conflicts
- Worktree deletion failures
- Unauthorized filesystem access
- Malicious instructions inside source material

### End-to-end tests

Use a dedicated test repository.

Prove the main workflow with a real or explicitly controlled test agent integration.

Do not present mocked integration tests as proof that live provider interoperability works.

### Performance

Minimize idle CPU/RAM consumption.

Avoid wasteful polling.

Do not reindex complete repositories on every session start.

Use incremental file watchers and bounded queues where appropriate.

Do not store enormous uncompressed agent transcripts without retention controls.

---

# PART X — CROSS-MODEL ENGINEERING

## 21. GPT-6 Astra High as Independent Engineering Reviewer

You are the lead developer.

You may occasionally consult my locally installed OpenAI Codex CLI using **GPT-6 Astra at High reasoning effort**.

The purpose is deeper engineering scrutiny, not unnecessary model consumption.

I have a ChatGPT Plus subscription with Codex access.

### Integration procedure

Inspect the installed CLI version, authentication, and supported flags.

Use the existing authenticated Codex CLI where available.

Prefer a bounded, read-only consultation similar to:

`codex exec -m gpt-6-astra -c 'model_reasoning_effort="high"' --sandbox read-only --ephemeral "SPECIFIC TECHNICAL REVIEW REQUEST"`

Confirm command compatibility with the installed CLI.

If invoked from PowerShell, use correctly quoted arguments.

Do not expose credentials.

Do not switch to separately billed API authentication.

If Astra is unavailable or the subscription allowance is exhausted, continue independently.

### When to consult Astra

Use it for genuinely difficult or consequential problems:

1. Initial architecture and provider integration strategy
2. Memory system provenance, retrieval, and consistency design
3. Safe subprocess/worktree orchestration and concurrency
4. Hook execution security and permission boundaries
5. A critical persistent bug
6. Independent reliability/security review of completed P0 systems

Limit initial development to approximately 4–6 substantial consultations unless a serious engineering issue justifies more.

### Review protocol

For each consultation:

- Give Astra a sharply defined question.
- Supply relevant files and constraints.
- Ask for concrete failure modes.
- Request tradeoff analysis.
- Ask for the smallest robust correction.
- Evaluate the answer independently.
- Implement and validate adopted recommendations.

Do not blindly accept either model's proposals.

Maintain `ENGINEERING_REVIEWS.md` with significant findings and validation outcomes.

### Responsibility

Claude remains the implementing engineer.

Astra is an occasional independent specialist.

No overlapping edits to the same files.

No uncontrolled multi-agent loops.

---

# PART XI — PROJECT CONTINUITY

## 22. Persistent Development State

Maintain:

`PROGRESS.md`
- Completed milestones
- Active implementation
- Known issues
- Next actions
- Actual test results

`DECISIONS.md`
- Significant architecture choices
- Alternatives considered
- Reasons for chosen approach

`ENGINEERING_REVIEWS.md`
- Independent reviews
- Findings
- Resolutions

`ROADMAP.md`
- MVP versus later features
- Explicit priorities
- Deferred work

Update these as development progresses.

Avoid filling the root repository with dozens of repetitive planning documents.

If context becomes limited, preserve actionable state before continuing.

Use subagents selectively for independent research, testing, UI work, and code review when doing so is useful.

Do not create subagents simply because parallelism is available.

---

# PART XII — PRODUCT ACCEPTANCE

## 23. Definition of a Successful First Release

FORGE V1 is successful when a real developer can:

1. Install and launch the application on Windows.
2. Register an existing local Git repository.
3. Create a meaningful engineering mission.
4. Use an installed supported coding agent.
5. Observe real agent progress.
6. Approve or reject consequential actions.
7. Inspect source-code changes.
8. Run verification checks.
9. Determine which acceptance criteria passed.
10. Review and safely merge approved work.
11. Save important engineering discoveries.
12. Reuse relevant project knowledge in a later mission.
13. Inspect their persistent history after restarting FORGE.
14. Use the essential workflows without a cloud account.

For later milestones, verify a working Obsidian integration and Harness Studio separately before declaring them complete.

Do not claim the enterprise edition exists until its security and multiuser infrastructure have actually been implemented and tested.

---

# PART XIII — FINAL INSTRUCTIONS

Build FORGE to be genuinely competitive as an engineering product.

Its value must come from:

**Execution + Memory + Context + Verification + Control.**

Not from having the most dashboards.

Not from launching the most simultaneous agents.

Not from automatically injecting giant prompts.

Not from creating more complicated abstractions than necessary.

The long-term ambition is substantial: developers and engineering organizations should be able to use FORGE as their primary environment for directing, understanding, evaluating, and continuously improving AI-assisted software development.

But achieve this by shipping excellent, reliable functionality incrementally.

### Non-negotiable standards

- Real provider integrations
- Real Git operations
- Real verification results
- Persistent and inspectable knowledge
- Secure approval boundaries
- Recoverable session state
- Excellent UI/UX
- No fake product functionality
- No uncontrolled inference spending
- No hidden API billing
- No unnecessary rewrites of existing coding harnesses
- No claims of security or enterprise readiness without evidence

### Your immediate assignment

1. Research and verify relevant APIs and integration surfaces.
2. Define a practical and elegant V1 architecture.
3. Start implementing Phase 1 immediately.
4. Test the integration against a real local repository.
5. Continue through the next phases as practical while maintaining working builds.
6. Report precisely what is complete and what remains unverified.

Do not respond with just a theoretical plan.

Do not produce a pretty static demo and call it finished.

I want working software that becomes more useful and more intelligent as I continue building projects with it.

**Build the engineering cockpit that AI-native developers will actually want open every day.**
