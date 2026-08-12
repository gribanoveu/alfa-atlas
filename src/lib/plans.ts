import { invoke } from "@tauri-apps/api/core";

/** Mirrors `domain::plan::PlanTodoStatus`. */
export type PlanTodoStatus = "pending" | "inProgress" | "completed" | "cancelled";

/** Mirrors `domain::plan::PlanTodo`. */
export type PlanTodo = {
  id: string;
  content: string;
  status: PlanTodoStatus;
  note: string | null;
};

/** Mirrors `domain::plan::PlanSummary`. */
export type PlanSummary = {
  id: string;
  name: string;
  overview: string;
  todoTotal: number;
  todoCompleted: number;
  createdAtMs: number;
  updatedAtMs: number;
};

/** Mirrors `domain::plan::PlanRecord`. */
export type PlanRecord = {
  id: string;
  name: string;
  overview: string;
  plan: string;
  todos: PlanTodo[];
  createdAtMs: number;
  updatedAtMs: number;
  chatId: string | null;
  repoRoot: string | null;
};

export function planList(): Promise<PlanSummary[]> {
  return invoke<PlanSummary[]>("plan_list");
}

export function planGet(planId: string): Promise<PlanRecord> {
  return invoke<PlanRecord>("plan_get", { planId });
}

export function planDelete(planId: string): Promise<void> {
  return invoke<void>("plan_delete", { planId });
}
