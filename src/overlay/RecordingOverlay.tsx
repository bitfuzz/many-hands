import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MicrophoneIcon,
  TranscriptionIcon,
  CancelIcon,
  PauseIcon,
  PlayIcon,
} from "../components/icons";
import "./RecordingOverlay.css";
import { commands } from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";

type OverlayState =
  | "recording"
  | "transcribing"
  | "processing"
  | "meeting"
  | "meeting_paused";

const RECORDING_BAR_COUNT = 9;
const MEETING_BAR_COUNT = 16;
const LEVEL_BUFFER_COUNT = 20;

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  const [levels, setLevels] = useState<number[]>(Array(LEVEL_BUFFER_COUNT).fill(0));
  const smoothedLevelsRef = useRef<number[]>(Array(LEVEL_BUFFER_COUNT).fill(0));
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      // Listen for show-overlay event from Rust
      const unlistenShow = await listen("show-overlay", async (event) => {
        // Sync language from settings each time overlay is shown
        await syncLanguageFromSettings();
        const overlayState = event.payload as OverlayState;
        setState(overlayState);
        setIsVisible(true);
      });

      // Listen for hide-overlay event from Rust
      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });

      // Listen for mic-level updates
      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];

        // Apply smoothing to reduce jitter
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3; // Smooth transition
        });

        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, LEVEL_BUFFER_COUNT));
      });

      // Cleanup function
      return () => {
        unlistenShow();
        unlistenHide();
        unlistenLevel();
      };
    };

    setupEventListeners();
  }, []);

  const getIcon = () => {
    if (state === "recording") {
      return <MicrophoneIcon />;
    } else {
      return <TranscriptionIcon />;
    }
  };

  const recordingLevels = levels.slice(0, RECORDING_BAR_COUNT);
  const meetingLevels = levels.slice(0, MEETING_BAR_COUNT);

  const isMeetingPaused = state === "meeting_paused";
  const isMeetingOverlay = state === "meeting" || state === "meeting_paused";

  const toggleMeetingPause = () => {
    void commands.toggleMeetingRecordingPause();
  };

  return (
    <div
      dir={direction}
      className={`recording-overlay ${isVisible ? "fade-in" : ""} ${isMeetingOverlay ? "meeting-overlay" : ""}`}
    >
      {isMeetingOverlay ? (
        <div className={`meeting-fab ${isMeetingPaused ? "is-paused" : ""}`}>
          <button
            type="button"
            className="meeting-pause-button"
            onClick={toggleMeetingPause}
            aria-label={
              isMeetingPaused
                ? t("tray.startMeetingRecording")
                : t("tray.stopMeetingRecording")
            }
          >
            {isMeetingPaused ? <PlayIcon /> : <PauseIcon />}
          </button>

          <div className="meeting-pill-visualizer" aria-hidden="true">
            {meetingLevels.map((v, i) => (
              <div
                key={i}
                className="meeting-pill-bar"
                style={{
                  height: `${Math.min(16, 3 + Math.pow(v, 0.72) * 12)}px`,
                  opacity: Math.max(0.22, v * 1.6),
                  transition: "height 70ms ease-out, opacity 120ms ease-out",
                }}
              />
            ))}
          </div>
        </div>
      ) : (
        <>
          <div className="overlay-left">{getIcon()}</div>

          <div className="overlay-middle">
            {state === "recording" && (
              <div className="bars-container">
                {recordingLevels.map((v, i) => (
                  <div
                    key={i}
                    className="bar"
                    style={{
                      height: `${Math.min(20, 4 + Math.pow(v, 0.7) * 16)}px`,
                      transition: "height 60ms ease-out, opacity 120ms ease-out",
                      opacity: Math.max(0.2, v * 1.7),
                    }}
                  />
                ))}
              </div>
            )}
            {state === "transcribing" && (
              <div className="transcribing-text">{t("overlay.transcribing")}</div>
            )}
            {state === "processing" && (
              <div className="transcribing-text">{t("overlay.processing")}</div>
            )}
          </div>

          <div className="overlay-right">
            {state === "recording" && (
              <div
                className="cancel-button"
                onClick={() => {
                  commands.cancelOperation();
                }}
              >
                <CancelIcon />
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
};

export default RecordingOverlay;
