#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
COMFY=${OPENCHATCUT_COMFYUI_ROOT:-$ROOT/runtime/comfyui}
HUGGINGFACE_BASE_URL=${HUGGINGFACE_BASE_URL:-https://huggingface.co}
MIN_FREE_KIB=$((180 * 1024 * 1024))

if [[ ! -d "$COMFY/models" ]]; then
  echo "ComfyUI is not installed at $COMFY" >&2
  exit 2
fi

available_kib=$(df -Pk "$ROOT" | awk 'NR == 2 { print $4 }')
if (( available_kib < MIN_FREE_KIB )); then
  echo "At least 180 GiB of free space is required for the pinned model set" >&2
  exit 2
fi

download() {
  local relative=$1
  local url=$2
  local output="$COMFY/models/$relative"
  local partial="$output.part"
  mkdir -p "$(dirname "$output")"
  if [[ -s "$output" ]]; then
    echo "present: $relative"
    return
  fi
  echo "download: $relative"
  curl \
    --fail \
    --location \
    --retry 8 \
    --retry-all-errors \
    --connect-timeout 20 \
    --continue-at - \
    --output "$partial" \
    "$url"
  mv "$partial" "$output"
}

QWEN_REV=46839d338df81ce625d5fae27d7e370314c0fbc9
QWEN_EDIT_REV=e9e85de74a8f48c1e3e2656617626348675a2f21
QWEN_LIGHTNING_REV=a52649c9d0f6e1a248bff13f0df33bb8a2abdb52
WAN21_REV=06e001fc51048fb03433a6fb25334de7836704a5
WAN22_REV=fb1388adc906ab39ffc26ee40e96b22886b56bc4
ACE_REV=54b2ef4d8af5582f54c7e6b84c22b679a194bc4b

download \
  text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Qwen-Image_ComfyUI/resolve/$QWEN_REV/split_files/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors"
download \
  vae/qwen_image_vae.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Qwen-Image_ComfyUI/resolve/$QWEN_REV/split_files/vae/qwen_image_vae.safetensors"
download \
  diffusion_models/qwen_image_2512_fp8_e4m3fn.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Qwen-Image_ComfyUI/resolve/$QWEN_REV/split_files/diffusion_models/qwen_image_2512_fp8_e4m3fn.safetensors"
download \
  diffusion_models/qwen_image_edit_2511_bf16.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Qwen-Image-Edit_ComfyUI/resolve/$QWEN_EDIT_REV/split_files/diffusion_models/qwen_image_edit_2511_bf16.safetensors"
download \
  loras/Qwen-Image-2512-Lightning-4steps-V1.0-fp32.safetensors \
  "$HUGGINGFACE_BASE_URL/lightx2v/Qwen-Image-2512-Lightning/resolve/$QWEN_LIGHTNING_REV/Qwen-Image-2512-Lightning-4steps-V1.0-fp32.safetensors"

download \
  text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/$WAN21_REV/split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"
download \
  vae/wan_2.1_vae.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/$WAN22_REV/split_files/vae/wan_2.1_vae.safetensors"
download \
  diffusion_models/wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/$WAN22_REV/split_files/diffusion_models/wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors"
download \
  diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/$WAN22_REV/split_files/diffusion_models/wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors"

download \
  diffusion_models/acestep_v1.5_turbo.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/ace_step_1.5_ComfyUI_files/resolve/$ACE_REV/split_files/diffusion_models/acestep_v1.5_turbo.safetensors"
download \
  text_encoders/qwen_0.6b_ace15.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/ace_step_1.5_ComfyUI_files/resolve/$ACE_REV/split_files/text_encoders/qwen_0.6b_ace15.safetensors"
download \
  vae/ace_1.5_vae.safetensors \
  "$HUGGINGFACE_BASE_URL/Comfy-Org/ace_step_1.5_ComfyUI_files/resolve/$ACE_REV/split_files/vae/ace_1.5_vae.safetensors"

echo "Pinned ComfyUI model set is present."
