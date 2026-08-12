import { invoke } from "@tauri-apps/api/core";

export type RuleDef = {
  id: string;
  title: string;
  weight: number;
  defaultEnabled: boolean;
};

export type Finding = {
  ruleId: string;
  title: string;
  passed: boolean;
  weight: number;
  message: string;
};

export type FolderReport = {
  folder: string;
  methodName: string;
  score: number;
  maxScore: number;
  passed: boolean;
  findings: Finding[];
};

export type StandardsReport = {
  folders: FolderReport[];
  overallPassed: boolean;
  checkedAt: number;
};

export type StandardsRuleConfig = {
  rules: Record<string, boolean>;
};

export function getStandardsRules(): Promise<RuleDef[]> {
  return invoke<RuleDef[]>("get_standards_rules");
}

export function getStandardsConfig(): Promise<StandardsRuleConfig> {
  return invoke<StandardsRuleConfig>("get_standards_config");
}

export function setStandardsConfig(
  config: StandardsRuleConfig,
): Promise<void> {
  return invoke<void>("set_standards_config", { config });
}

export function checkStandards(docsRoot: string): Promise<StandardsReport> {
  return invoke<StandardsReport>("check_standards", { docsRoot });
}
