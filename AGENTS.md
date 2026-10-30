# HarflowEngine

Read README.md and docs/architecture.md before editing. Preserve layer boundaries and existing work. Target files below 250 substantive lines and functions below 50 code lines; enforced maxima are 400 and 80. Run the documented checks before completion. Delegated workers follow their assigned scope and do not restart orchestration or review loops. Professional artifacts remain neutral.

## Ownership and interfaces

hf-domain owns pure values; hf-application owns ports/use cases; hf-storage, hf-backends and hf-platform own concrete adapters; hf-interfaces owns transport mapping; hf-host owns composition. hf-contracts remains an empty seam while HarflowMCP owns canonical contracts. hf-architecture is build-time policy tooling. Keep platform effects out of domain/application and preserve the explicit Cargo dependency inventory.

## Test ownership

When unit tests are needed, the primary delegates them to the dedicated test_writer (gpt-6-luna, medium effort). That worker owns tests/ and coordinates production interfaces with the implementer. Preserve concurrent edits. Delegated workers do not restart the surrounding review workflow.
