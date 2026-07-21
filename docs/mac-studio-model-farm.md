# Mac Studio local model farm

This deployment keeps OpenChatCut project state on the editing workstation and
uses a Mac Studio as an inference node. New API is the only public gateway.
LM Studio, ComfyUI, transcription, speech, music, SFX, and denoise services stay
on loopback or a private container network.

## Verified node

- Host: `100.125.246.22` over the existing private network.
- Hardware: Apple M3 Ultra, 32 CPU cores, 80 GPU cores, 512 GiB unified memory.
- Model root: `/Volumes/External/openchatcut-models`.
- LM Studio model link:
  `~/.lmstudio/models -> /Volumes/External/openchatcut-models/lm-studio`.
- Public gateway: `https://api.singularity-x.ai:9443/v1`.

Do not place new model weights on the internal disk. Do not expose LM Studio,
ComfyUI, or individual Python inference servers directly to the Internet.

## Stable aliases

New API model mapping must expose stable aliases so OpenChatCut configuration
does not depend on a particular quantization or runtime:

| Alias | Capability | Initial backend |
| --- | --- | --- |
| `occ-edit-planner` | semantic edit planning | `qwen/qwen3-coder-next` in LM Studio |
| `occ-edit-fast` | low-latency mechanical planning | `zai-org/glm-4.7-flash` in LM Studio |
| `occ-vision` | representative-frame/contact-sheet analysis | `gemma-4-31b-it` initially |
| `occ-embedding` | asset semantic search | Nomic Embed Text v1.5 in LM Studio |
| `occ-rerank` | B-roll reranking | BGE Reranker v2 M3 service |
| `occ-asr` | word timestamps and optional diarization | WhisperX |
| `occ-image` | image generation/editing | ComfyUI workflow |
| `occ-video` | asynchronous video generation | ComfyUI workflow |
| `occ-tts` | voiceover | Qwen3-TTS, Kokoro/Piper fallback |
| `occ-music` | music generation | ACE-Step workflow |
| `occ-sfx` | sound-effect generation | AudioGen-compatible workflow |
| `occ-denoise` | reversible dialogue cleanup | DeepFilterNet |

Only load the models needed by active jobs. The node has enough memory for very
large models, but Metal compute and memory bandwidth remain shared resources.
`load-lm-studio-models.sh` keeps the planner, fast-edit, and small embedding
model hot. Vision uses LM Studio just-in-time loading. Reranking runs in a
separate authenticated service because the Jina GGUF files are classified as
chat models by LM Studio 0.4.7 and do not implement `/v1/rerank`. Separating the
endpoint also prevents a vision-model load failure from the always-hot editing
path.

## Reproducible installation

The scripts in `scripts/mac-studio-model-farm/` keep runtime environments and
weights on the external model volume:

```sh
scripts/mac-studio-model-farm/install-native-runtimes.sh
scripts/mac-studio-model-farm/install-rerank-runtime.sh
scripts/mac-studio-model-farm/install-asr-service.sh
scripts/mac-studio-model-farm/download-asr-model.sh
scripts/mac-studio-model-farm/install-tts-service.sh
scripts/mac-studio-model-farm/download-qwen-tts-models.sh
scripts/mac-studio-model-farm/install-image-service.sh
scripts/mac-studio-model-farm/download-comfy-models.sh
scripts/mac-studio-model-farm/comfyui-service.sh start
scripts/mac-studio-model-farm/rerank-service.sh start
scripts/mac-studio-model-farm/asr-service.sh start
scripts/mac-studio-model-farm/tts-service.sh start
scripts/mac-studio-model-farm/image-service.sh start
scripts/mac-studio-model-farm/load-lm-studio-models.sh
scripts/mac-studio-model-farm/status.sh
scripts/mac-studio-model-farm/configure-openchatcut-new-api.sh
scripts/mac-studio-model-farm/smoke-new-api.py
scripts/mac-studio-model-farm/smoke-image.py \
  --url https://api.singularity-x.ai:9443/v1/images/generations \
  --keychain-service singularity-x-new-api-token
```

The runtime installer pins WhisperX 3.8.6, DeepFilterNet 0.5.6, and qwen-tts
0.1.1. The Qwen downloader installs the 1.7B CustomVoice, Base, and VoiceDesign
models so normal narration, reference-audio voice cloning, and text-directed
voice design do not share a remote dependency. WhisperX runs on macOS with
`--device cpu --compute_type int8`; speaker diarization additionally requires
the user to accept the pyannote model terms and provide a Hugging Face token.

The ComfyUI downloader is revision-pinned and resumable. When direct access to
Hugging Face is unavailable, select an operator-approved mirror for that single
process without editing the pinned repository paths:

```sh
HUGGINGFACE_BASE_URL=https://hf-mirror.com \
  scripts/mac-studio-model-farm/download-comfy-models.sh
```

The multilingual BGE reranker snapshot is downloaded from its Apache-2.0
ModelScope mirror. Its loopback service implements the Jina-compatible
`/v1/rerank` contract and requires a randomly generated, mode-`0600` internal
token. New API is the only client that receives that token.

WhisperX is exposed internally through an authenticated OpenAI-compatible
`/v1/audio/transcriptions` endpoint on `127.0.0.1:8191`. It uses the Silero VAD
path so basic transcription does not depend on gated pyannote weights. The
service returns `verbose_json` segment and real aligned word timestamps when a
language alignment model is available; it reports an alignment warning rather
than inventing word timing when that optional model cannot be loaded.
The CTranslate2 `large-v3` checkpoint is downloaded from the ModelScope
`gpustack/faster-whisper-large-v3` mirror and loaded from an absolute local
path. This avoids making a transcription job depend on Hugging Face metadata
requests at runtime.

Qwen3-TTS CustomVoice is exposed internally through the authenticated
OpenAI-compatible `/v1/audio/speech` endpoint on `127.0.0.1:8192`. The service
loads the local 1.7B checkpoint lazily, maps the standard OpenAI voice names to
editable Qwen speakers, and uses FFmpeg only for requested output encoding or
speed adjustment. It never accepts remote reference-audio URLs.

Qwen Image 2512 is exposed internally through an authenticated
OpenAI-compatible `/v1/images/generations` endpoint on `127.0.0.1:8193`.
The service submits a fixed, editable ComfyUI graph using the pinned FP8 model,
Qwen 2.5 VL text encoder, Lightning four-step LoRA, and Qwen image VAE. It
accepts bounded prompts and an allowlist of image sizes, returns only base64
PNG/JPEG data, deletes the transient ComfyUI output after retrieval, and never
accepts remote resource URLs. New API maps `occ-image` to this service.

## Credentials

Never commit keys. On the OpenChatCut workstation, create the New API token
slot interactively:

```sh
security add-generic-password -U \
  -a openchatcut \
  -s singularity-x-new-api-token \
  -w
```

On the Mac Studio, create the LM Studio upstream token slot:

```sh
security add-generic-password -U \
  -a openchatcut \
  -s lm-studio-api-token \
  -w
```

The workstation provider file can then reference Keychain without containing a
secret:

```json
{
  "openaiCompatible": {
    "baseUrl": "https://api.singularity-x.ai:9443/v1",
    "model": "occ-edit-planner",
    "apiKeyKeychain": {
      "account": "openchatcut",
      "service": "singularity-x-new-api-token"
    }
  }
}
```

Keep the file mode at `0600`. Generation adapters are added to the same private
file only after their New API aliases pass direct smoke tests. Once the
workstation Keychain slot exists, `configure-openchatcut-new-api.sh` merges the
planner adapter into `~/.openchatcut/providers.json` without placing the token
in that file. Set `OPENCHATCUT_ENABLE_NEW_API_VIDEO=1` only after the
`occ-video` asynchronous backend has passed its real generation smoke test.
`smoke-new-api.py` reads the token from Keychain and verifies authenticated
planner, embedding, rerank, TTS, and word-aligned ASR inference without
printing the credential. `smoke-image.py` performs an explicit real image
generation smoke because it is slower and more resource intensive. The
workstation configurator enables the verified `newApiVoice`, `newApiAsr`, and
`newApiImage` adapters by default. Set `OPENCHATCUT_ENABLE_NEW_API_TTS=0`,
`OPENCHATCUT_ENABLE_NEW_API_ASR=0`, or
`OPENCHATCUT_ENABLE_NEW_API_IMAGE=0` to omit one of those private providers.

## Rollout gates

1. Model storage migration is verified by a clean second `rsync` and `lms ls`.
2. Every native runtime runs in its own virtual environment under the external
   model root. ComfyUI, WhisperX, and DeepFilterNet use Python 3.11; Qwen3-TTS
   uses Python 3.12.
3. Direct loopback smoke tests pass before a New API channel is created.
4. New API aliases pass authenticated `/v1/models` and capability-specific
   calls without exposing an upstream URL.
5. OpenChatCut validates the returned media, downloads it into its managed
   content store, and records provider/model/prompt/seed provenance.
6. The internal-disk LM Studio rollback copy is deleted only after end-to-end
   inference succeeds from the external model root.

## macOS service note

ComfyUI listens on `127.0.0.1:8188`. The checked-in launch-agent template uses
the ComfyUI virtual environment rather than Homebrew's system interpreter.
macOS may still prevent a background launch agent from reading an external
volume until the Python executable has been granted the relevant Files and
Folders or Full Disk Access permission. Do not broaden the listener address to
work around this restriction.

Until that permission is granted, start ComfyUI from an authorized interactive
session with `comfyui-service.sh start`; it detaches the process with `nohup`.
The runtime, caches, outputs, database, and weights remain under
`/Volumes/External/openchatcut-models`; logs go to
`~/Library/Logs/OpenChatCut`. Verify the service locally on the Mac Studio with:

```sh
curl --fail http://127.0.0.1:8188/system_stats
```

The service is not considered reboot-persistent until a fresh-login launchd
test succeeds while the external volume is mounted.
