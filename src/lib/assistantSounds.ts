import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import needAnswerUrl from "../assets/sounds/need_answer.mp3";
import taskDoneUrl from "../assets/sounds/task_done.mp3";

/** Lazily-created players, one per sound — reused so a rapid sequence of
 * events rewinds and replays instead of stacking overlapping Audio elements. */
let taskDoneAudio: HTMLAudioElement | null = null;
let needAnswerAudio: HTMLAudioElement | null = null;

function play(audio: HTMLAudioElement | null, url: string): HTMLAudioElement {
  const player = audio ?? new Audio(url);
  player.currentTime = 0;
  void player.play().catch(() => {
    // Autoplay may be blocked before any user gesture, or the file may fail
    // to decode — never surface that into the chat UI.
  });
  return player;
}

/** OS banner when the main window is not focused. Sound still plays either
 * way — the banner is only useful when the user isn't already looking. */
async function sendOsNotification(title: string, body: string): Promise<void> {
  try {
    if (await getCurrentWindow().isFocused()) return;

    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (!granted) return;

    sendNotification({ title, body });
  } catch {
    // Plugin missing / permission denied / window API failure — never
    // surface into the chat UI.
  }
}

/** Chime (+ OS notification if unfocused) when an assistant turn finishes
 * successfully. Caller gates on `LlmSettings.taskDoneSoundEnabled`. */
export function playTaskDoneSound(): void {
  taskDoneAudio = play(taskDoneAudio, taskDoneUrl);
  void sendOsNotification("Ассистент", "Работа завершена");
}

/** Chime (+ OS notification if unfocused) when an `askUser` card appears.
 * Caller gates on `LlmSettings.needAnswerSoundEnabled`. */
export function playNeedAnswerSound(): void {
  needAnswerAudio = play(needAnswerAudio, needAnswerUrl);
  void sendOsNotification("Ассистент", "Нужен ваш ответ");
}
