# 内置提示音效来源与授权

本目录下的全部音效来自 Kenney 的 **Interface Sounds (1.0)** 素材包，授权为
[Creative Commons Zero (CC0 1.0)](http://creativecommons.org/publicdomain/zero/1.0/)——
可自由用于个人、教育与商业项目，无需署名（本项目仍在此致谢）。

来源：<https://kenney.nl/assets/interface-sounds>
镜像仓库（WAV 格式）：<https://github.com/Calinou/kenney-interface-sounds>

转换处理：ffmpeg 裁剪首尾静音（-45dB 阈值）、混为单声道、44.1kHz / 16-bit PCM WAV。

本目录只保留 macOS 系统音兜底实际用到的 3 个 WAV（音色按 cometix-dictation 的
极简提示音风格，从同一素材包挑选的近似替代——Apple 系统音不可再分发，macOS 上
直接运行时引用系统音 `Tink`/`Pop`/`Basso`，与 cometix-dictation 一致）：

| 文件 | 原始文件 | 时长 | 兜底对象 | 默认用途 |
| --- | --- | --- | --- | --- |
| `glass.wav` | `glass_005.wav` | ~0.04s | 系统音 `Glass` | 录音开始（Windows/系统音不可用）·玻璃轻碰音色，按用户试听从「金属叮」家族候选中选定；非 macOS 默认开始音即 Glass，名称与实际发声一致 |
| `pop.wav` | `drop_003.wav` | ~0.07s | 系统音 `Pop` | 进入识别处理（Windows/系统音不可用） |
| `error.wav` | `error_001.wav` | ~0.13s | 系统音 `Basso` | 识别失败（默认不发声，可手动选择） |

> Kenney 的原始授权声明见下（摘自素材包 LICENSE.txt）：
>
> License: (Creative Commons Zero, CC0)
> <http://creativecommons.org/publicdomain/zero/1.0/>
>
> This content is free to use in personal, educational and commercial projects.
> Support us by crediting Kenney or www.kenney.nl (this is not mandatory)
