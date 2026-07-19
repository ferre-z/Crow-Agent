import {
  formatSkillsForSystemPrompt,
  loadSkills,
  type ExecutionEnv,
  type Skill,
  type SkillDiagnostic,
} from "@earendil-works/pi-agent-core";

/**
 * Load SKILL.md skills from the given directories. Never throws: missing dirs
 * are skipped and malformed skills come back in `diagnostics` for the caller
 * to log.
 */
export async function loadCrowSkills(
  env: ExecutionEnv,
  skillDirs: string[],
): Promise<{ skills: Skill[]; diagnostics: SkillDiagnostic[] }> {
  return loadSkills(env, skillDirs);
}

/** Base prompt plus the agentskills.io XML block when any skills are loaded. */
export function buildSystemPrompt(base: string, skills: Skill[]): string {
  if (skills.length === 0) return base;
  return base + "\n\n" + formatSkillsForSystemPrompt(skills);
}
