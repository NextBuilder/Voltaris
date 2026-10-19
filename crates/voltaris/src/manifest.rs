//! Skill and agent manifests embedded at compile time.
//!
//! The client-aware `init` subcommand installs shared skills for Claude or Codex.
//! Claude-specific agents and hooks remain scoped to Claude's directories.
//! Hook skills are also patched into `~/.claude/settings.json`.

/// A skill to install to `~/.claude/skills/<name>/SKILL.md`.
/// Optional reference files go into `~/.claude/skills/<name>/references/`.
pub struct SkillManifest {
    pub name: &'static str,
    pub content: &'static str,
    pub references: &'static [(&'static str, &'static str)],
}

/// An agent to install to `~/.claude/agents/<filename>`.
pub struct AgentManifest {
    pub filename: &'static str,
    pub content: &'static str,
}

/// A hook-bound skill: triggers before/after specific MCP tool calls.
/// Installed as a hook entry in `~/.claude/settings.json` that runs
/// `voltaris.exe hook <name>` to emit Claude's structured hook JSON.
pub struct HookSkillManifest {
    pub name: &'static str,
    pub content: &'static str,
    pub board_access: voltaris_core::tools::BoardAccess,
    pub event: &'static str, // "PreToolUse" or "PostToolUse"
}

// ─── Skills ──────────────────────────────────────────────────────────────────

pub const SKILLS: &[SkillManifest] = &[
    SkillManifest {
        name: "voltaris",
        content: include_str!("../assets/skills/voltaris/SKILL.md"),
        references: &[(
            "reliability-contract.md",
            include_str!("../../../docs/dev/RELIABILITY_CONTRACT.md"),
        )],
    },
    SkillManifest {
        name: "kicad-schematic",
        content: include_str!("../assets/skills/kicad-schematic/SKILL.md"),
        references: &[
            (
                "common-lib-ids.md",
                include_str!("../assets/skills/kicad-schematic/references/common-lib-ids.md"),
            ),
            (
                "wiring-patterns.md",
                include_str!("../assets/skills/kicad-schematic/references/wiring-patterns.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-pcb",
        content: include_str!("../assets/skills/kicad-pcb/SKILL.md"),
        references: &[
            (
                "layer-reference.md",
                include_str!("../assets/skills/kicad-pcb/references/layer-reference.md"),
            ),
            (
                "trace-width-table.md",
                include_str!("../assets/skills/kicad-pcb/references/trace-width-table.md"),
            ),
            (
                "design-rules.md",
                include_str!("../assets/skills/kicad-pcb/references/design-rules.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-manufacture",
        content: include_str!("../assets/skills/kicad-manufacture/SKILL.md"),
        references: &[
            (
                "jlcpcb-rules.md",
                include_str!("../assets/skills/kicad-manufacture/references/jlcpcb-rules.md"),
            ),
            (
                "gerber-layers.md",
                include_str!("../assets/skills/kicad-manufacture/references/gerber-layers.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-review",
        content: include_str!("../assets/skills/kicad-review/SKILL.md"),
        references: &[
            (
                "error-taxonomy.md",
                include_str!("../assets/skills/kicad-review/references/error-taxonomy.md"),
            ),
            (
                "design-checklist.md",
                include_str!("../assets/skills/kicad-review/references/design-checklist.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-pro-workflow",
        content: include_str!("../assets/skills/kicad-pro-workflow/SKILL.md"),
        references: &[
            (
                "datasheet-rules.md",
                include_str!("../assets/skills/kicad-pro-workflow/references/datasheet-rules.md"),
            ),
            (
                "expert-checklist.md",
                include_str!("../assets/skills/kicad-pro-workflow/references/expert-checklist.md"),
            ),
            (
                "bringup-plan.md",
                include_str!("../assets/skills/kicad-pro-workflow/references/bringup-plan.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-power-design",
        content: include_str!("../assets/skills/kicad-power-design/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-high-speed-rf",
        content: include_str!("../assets/skills/kicad-high-speed-rf/SKILL.md"),
        references: &[(
            "impedance-guide.md",
            include_str!("../assets/skills/kicad-high-speed-rf/references/impedance-guide.md"),
        )],
    },
    SkillManifest {
        name: "kicad-esp32",
        content: include_str!("../assets/skills/kicad-esp32/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-mcu-design",
        content: include_str!("../assets/skills/kicad-mcu-design/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-part-sourcing",
        content: include_str!("../assets/skills/kicad-part-sourcing/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-simulation",
        content: include_str!("../assets/skills/kicad-simulation/SKILL.md"),
        references: &[(
            "recipes.md",
            include_str!("../assets/skills/kicad-simulation/references/recipes.md"),
        )],
    },
    SkillManifest {
        name: "kicad-library",
        content: include_str!("../assets/skills/kicad-library/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-pro-layout",
        content: include_str!("../assets/skills/kicad-pro-layout/SKILL.md"),
        references: &[
            (
                "layer-playbook.md",
                include_str!("../assets/skills/kicad-pro-layout/references/layer-playbook.md"),
            ),
            (
                "layout-review.md",
                include_str!("../assets/skills/kicad-pro-layout/references/layout-review.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-board-craft",
        content: include_str!("../assets/skills/kicad-board-craft/SKILL.md"),
        references: &[
            (
                "connector-standards.md",
                include_str!(
                    "../assets/skills/kicad-board-craft/references/connector-standards.md"
                ),
            ),
            (
                "form-factors.md",
                include_str!("../assets/skills/kicad-board-craft/references/form-factors.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-product-board",
        content: include_str!("../assets/skills/kicad-product-board/SKILL.md"),
        references: &[
            (
                "reference-board-study.md",
                include_str!(
                    "../assets/skills/kicad-product-board/references/reference-board-study.md"
                ),
            ),
            (
                "circuit-blocks.md",
                include_str!("../assets/skills/kicad-product-board/references/circuit-blocks.md"),
            ),
            (
                "release-package.md",
                include_str!("../assets/skills/kicad-product-board/references/release-package.md"),
            ),
            (
                "scorecard.md",
                include_str!("../assets/skills/kicad-product-board/references/scorecard.md"),
            ),
        ],
    },
    SkillManifest {
        name: "kicad-pro-silkscreen",
        content: include_str!("../assets/skills/kicad-pro-silkscreen/SKILL.md"),
        references: &[(
            "brand-styles.md",
            include_str!("../assets/skills/kicad-pro-silkscreen/references/brand-styles.md"),
        )],
    },
    SkillManifest {
        name: "kicad-multilayer",
        content: include_str!("../assets/skills/kicad-multilayer/SKILL.md"),
        references: &[(
            "stackup-catalog.md",
            include_str!("../assets/skills/kicad-multilayer/references/stackup-catalog.md"),
        )],
    },
    SkillManifest {
        name: "kicad-power-electronics",
        content: include_str!("../assets/skills/kicad-power-electronics/SKILL.md"),
        references: &[],
    },
    SkillManifest {
        name: "kicad-design-masters",
        content: include_str!("../assets/skills/kicad-design-masters/SKILL.md"),
        references: &[(
            "world-corpus.md",
            include_str!("../assets/skills/kicad-design-masters/references/world-corpus.md"),
        )],
    },
];

// ─── Agents ──────────────────────────────────────────────────────────────────

pub const AGENTS: &[AgentManifest] = &[
    AgentManifest {
        filename: "kicad-design-review-agent.md",
        content: include_str!("../assets/agents/kicad-design-review-agent.md"),
    },
    AgentManifest {
        filename: "kicad-schematic-build-agent.md",
        content: include_str!("../assets/agents/kicad-schematic-build-agent.md"),
    },
    AgentManifest {
        filename: "kicad-pro-review-agent.md",
        content: include_str!("../assets/agents/kicad-pro-review-agent.md"),
    },
];

// ─── Hook Skills ─────────────────────────────────────────────────────────────

pub const HOOK_SKILLS: &[HookSkillManifest] = &[
    HookSkillManifest {
        name: "pre-pcb-ipc",
        content: "This operation is live-IPC-only. KiCad must be running with the exact requested .kicad_pcb board open. If IPC or board identity fails, ask the user to open that board and retry once; do not bypass the server-side guard or edit the board file behind KiCad.",
        board_access: voltaris_core::tools::BoardAccess::LiveOnly,
        event: "PreToolUse",
    },
    HookSkillManifest {
        name: "pre-pcb-fallback",
        content: "This operation prefers live KiCad IPC but has a guarded closed-board file fallback. Let the tool select the safe path. Never force a file edit while KiCad holds this board open, and treat the server-side board identity and revision checks as authoritative.",
        board_access: voltaris_core::tools::BoardAccess::LivePreferredWithFallback,
        event: "PreToolUse",
    },
    HookSkillManifest {
        name: "pre-pcb-closed",
        content: "This operation requires the requested board to be closed in KiCad because it edits the saved file directly. Ask the user to close that board before applying the change; do not bypass the server-side open-board refusal.",
        board_access: voltaris_core::tools::BoardAccess::ClosedBoardOnly,
        event: "PreToolUse",
    },
    HookSkillManifest {
        name: "pre-pcb-conditional",
        content: "This tool has different board-state requirements for planning and applying. Dry-run or planning is non-mutating; before apply, follow the tool description and returned plan exactly. Do not reuse a stale plan revision or bypass the server-side board-state guard.",
        board_access: voltaris_core::tools::BoardAccess::ApplyModeDependent,
        event: "PreToolUse",
    },
];
