---
name: nice-friend
description: Consult OpenAI Codex as a trusted friend for design review, architecture feedback, or technical advice. Use when the user asks to consult Codex or wants a second opinion.
argument-hint: [consultation topic or question]
---

# Nice Friend - Codex Design Consultation

Consult OpenAI Codex CLI (`codex exec`) as a trusted, knowledgeable friend for design review and technical feedback.

## How to use

1. Prepare context for Codex by reading relevant files (DESIGN.md, source code, etc.)
2. Construct a prompt that includes:
   - The project context (what trajix is, current architecture)
   - The specific question or area to review
   - A request for honest, constructive feedback as a friend would give
3. Run `codex exec` via Bash with the prepared prompt and relevant context
4. Present Codex's feedback to the user

## Prompt construction

When calling `codex exec`, use this pattern:

```bash
codex exec -s read-only "
You are a trusted friend and experienced software engineer reviewing a project.
Be honest, constructive, and specific. Point out both strengths and concerns.
If you see potential issues, suggest alternatives.

Project: [brief description]
Context: [relevant design/code excerpts]

Question: [the user's specific question or $ARGUMENTS]
"
```

## Guidelines

- Always pass relevant project files as context (pipe file contents or reference them)
- Use `-s read-only` sandbox mode since this is a consultation, not code generation
- Use `-C` to set the working directory to the project root so Codex can read files
- If the topic is broad (e.g., "review the design"), focus on architecture, trade-offs, and potential pitfalls
- Present Codex's response clearly, noting it as Codex's perspective
- If Codex raises valid concerns, discuss whether to act on them