# 23: Desktop 1.0

## Goal and prerequisites

The designer and the other platforms. Requires milestone 22 and feedback from the beta. Rescoped by [the agentic beta decision](../DECISIONS.md#2026-11-10-agentic-beta-direction).

## Deliverables

- **The designer:** describe the look, or drop a reference PDF or a photo of a book, and the agent writes the theme JSON while the pages reflow. The panels that exist stay for fine tuning.
- The remaining theme sections stay JSON in the panel. A section gets a hand-designed panel only when beta users ask for it.
- After it, in this order: the annotated PDF review round trip, math in LaTeX syntax, and CSL citation styles. Each gets its own brief when it starts.
- Linux and Windows builds with their spell checker backends, packaging, and IME checks.
- Decisions from the beta recorded in [decisions](../DECISIONS.md).

## Completion criteria

A theme made by the designer from a description and from a reference PDF, each inspected as pages and valid against the schema. Every schema property is reachable from a designed section or as JSON in the panel. Linux and Windows builds launch and pass their IME checks.
