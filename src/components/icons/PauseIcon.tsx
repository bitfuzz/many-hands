import React from "react";

interface PauseIconProps {
  width?: number;
  height?: number;
  color?: string;
  className?: string;
}

const PauseIcon: React.FC<PauseIconProps> = ({
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
        d="M8.25 6.5C8.25 5.94772 8.69772 5.5 9.25 5.5C9.80228 5.5 10.25 5.94772 10.25 6.5V17.5C10.25 18.0523 9.80228 18.5 9.25 18.5C8.69772 18.5 8.25 18.0523 8.25 17.5V6.5Z"
        fill={color}
      />
      <path
        d="M13.75 6.5C13.75 5.94772 14.1977 5.5 14.75 5.5C15.3023 5.5 15.75 5.94772 15.75 6.5V17.5C15.75 18.0523 15.3023 18.5 14.75 18.5C14.1977 18.5 13.75 18.0523 13.75 17.5V6.5Z"
        fill={color}
      />
    </svg>
  );
};

export default PauseIcon;
