# Handover 002: Shared ontime-core & Full Normalized Client Schema Parity

## Summary of Accomplishments
- **ontime-core Integration:** Linked local `packages/ontime-core` Rust crate directly into `apps/server-rs/Cargo.toml`.
- **Normalized Client Schema Parity:**
  - Implemented `/data/rundowns/current` to return the complete normalized `Rundown` payload expected by client-side validator `isValidRundown` (`{ id, title, order, flatOrder, entries: Record<string, OntimeEntry>, revision }`).
  - Implemented `/data/rundowns` GET & POST handlers returning exact `ProjectRundownsList` (`{ loaded, rundowns }`).
- **Issue Tracking:** Logged [.issues/001_rundown_schema_parity.md](file:///Users/ozone/ontime-rs/.issues/001_rundown_schema_parity.md).

## Code & Test Locations
- **Rust Worker Server:** [apps/server-rs/Cargo.toml](file:///Users/ozone/ontime-rs/apps/server-rs/Cargo.toml), [apps/server-rs/src/room.rs](file:///Users/ozone/ontime-rs/apps/server-rs/src/room.rs)
- **Shared Rust Core Crate:** [packages/ontime-core/src/types.rs](file:///Users/ozone/ontime-rs/packages/ontime-core/src/types.rs), [packages/ontime-core/src/engine.rs](file:///Users/ozone/ontime-rs/packages/ontime-core/src/engine.rs)

## How to Dev & Test
1. **Deploy Worker Backend:**
   ```bash
   cd apps/server-rs
   pnpm exec wrangler deploy
   ```
2. **Open Web App:**
   Access `https://ontime-client.example.pages.dev` to verify creating rundowns, adding events/cues, and inspecting `/editor`.
