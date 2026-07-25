import raw from "../../app.config.json";

export type AppConfig = {
  version: string;
  documentationUrl: string;
  feedbackUrl: string;
  updatesUrl: string;
};

export const appConfig: AppConfig = raw;
