import React from "react";
import { useTranslation } from "react-i18next";
import type { MeetingAudioSource as MeetingAudioSourceType } from "@/bindings";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";

interface MeetingAudioSourceProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const MeetingAudioSourceSelector: React.FC<MeetingAudioSourceProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const selectedSource =
      getSetting("meeting_audio_source") || "microphone_and_system";

    const options = [
      {
        value: "microphone_and_system",
        label: t(
          "settings.general.meeting.audioSource.options.microphoneAndSystem",
        ),
      },
      {
        value: "microphone_only",
        label: t("settings.general.meeting.audioSource.options.microphoneOnly"),
      },
      {
        value: "system_only",
        label: t("settings.general.meeting.audioSource.options.systemOnly"),
      },
    ];

    return (
      <SettingContainer
        title={t("settings.general.meeting.audioSource.title")}
        description={t("settings.general.meeting.audioSource.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={options}
          selectedValue={selectedSource}
          onSelect={(value: string) =>
            updateSetting(
              "meeting_audio_source",
              value as MeetingAudioSourceType,
            )
          }
          placeholder={t("settings.general.meeting.audioSource.placeholder")}
          disabled={isUpdating("meeting_audio_source")}
        />
      </SettingContainer>
    );
  });

MeetingAudioSourceSelector.displayName = "MeetingAudioSourceSelector";
