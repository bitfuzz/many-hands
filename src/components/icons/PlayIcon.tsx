import React from "react";

interface PlayIconProps {
  width?: number;
  height?: number;
  color?: string;
  className?: string;
}

const PlayIcon: React.FC<PlayIconProps> = ({
  width = 20,
  height = 20,
  color = "#FFE8F2",
  className = "",
}) => {
  return (
    <svg
      width={width}
      height={height}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <path
        d="M8.94337 6.47157C8.94337 5.67558 9.81748 5.19631 10.4844 5.62204L17.5354 10.1231C18.1493 10.515 18.1493 11.4083 17.5354 11.8002L10.4844 16.3012C9.81748 16.727 8.94337 16.2477 8.94337 15.4517V6.47157Z"
        fill={color}
      />
    </svg>
  );
};

export default PlayIcon;
