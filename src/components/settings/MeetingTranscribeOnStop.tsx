import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface MeetingTranscribeOnStopProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MeetingTranscribeOnStop: React.FC<MeetingTranscribeOnStopProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("meeting_transcribe_on_stop") ?? true;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(value) => updateSetting("meeting_transcribe_on_stop", value)}
        isUpdating={isUpdating("meeting_transcribe_on_stop")}
        label={t("settings.general.meeting.transcribeOnStop.label")}
        description={t("settings.general.meeting.transcribeOnStop.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

MeetingTranscribeOnStop.displayName = "MeetingTranscribeOnStop";
