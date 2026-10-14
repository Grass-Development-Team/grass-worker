export type SetupStage = "database" | "admin" | "site" | "node" | "storage" | "finish" | "complete";

export interface SetupState {
  stage: SetupStage;
  is_setup_mode: boolean;
}
