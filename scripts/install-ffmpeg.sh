#!/usr/bin/env sh
set -eu

if command -v ffmpeg >/dev/null 2>&1 && command -v ffprobe >/dev/null 2>&1; then
  echo "FFmpeg/ffprobe already available"
  exit 0
fi

case "$(uname -s 2>/dev/null || printf unknown)" in
  Darwin)
    if ! command -v brew >/dev/null 2>&1; then
      echo "FFmpeg is required. Install Homebrew from https://brew.sh and rerun setup." >&2
      exit 1
    fi
    brew install ffmpeg
    ;;
  Linux)
    if command -v apt-get >/dev/null 2>&1; then
      if [ "$(id -u)" -eq 0 ]; then
        apt-get update
        apt-get install -y ffmpeg
      elif command -v sudo >/dev/null 2>&1; then
        sudo apt-get update
        sudo apt-get install -y ffmpeg
      else
        echo "FFmpeg is required; install the ffmpeg package as root." >&2
        exit 1
      fi
    elif command -v dnf >/dev/null 2>&1; then
      sudo dnf install -y ffmpeg
    elif command -v pacman >/dev/null 2>&1; then
      sudo pacman -S --needed --noconfirm ffmpeg
    elif command -v zypper >/dev/null 2>&1; then
      sudo zypper --non-interactive install ffmpeg
    else
      echo "FFmpeg is required; install ffmpeg and ffprobe with your package manager." >&2
      exit 1
    fi
    ;;
  MINGW*|MSYS*|CYGWIN*)
    if command -v winget.exe >/dev/null 2>&1; then
      winget.exe install --id Gyan.FFmpeg --exact --accept-package-agreements --accept-source-agreements
    elif command -v choco.exe >/dev/null 2>&1; then
      choco.exe install ffmpeg -y
    else
      echo "FFmpeg is required. Install it with winget or Chocolatey, then reopen the shell." >&2
      exit 1
    fi
    ;;
  *)
    echo "Unsupported platform: install ffmpeg and ffprobe, then rerun setup." >&2
    exit 1
    ;;
esac

if ! command -v ffmpeg >/dev/null 2>&1 || ! command -v ffprobe >/dev/null 2>&1; then
  echo "FFmpeg was installed but is not yet on PATH; reopen the shell and rerun setup." >&2
  exit 1
fi
