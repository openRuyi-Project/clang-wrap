# LLVM IR 安装产物协议（共同发布边界）

## 1. 目的与适用范围

本文定义 `CMakeEmitLLVMIR` 与 `clang-wrap` 安装 LLVM IR 后，消费者可以依赖的**共同发布边界**。两者不是替代关系，故协议不要求相同的构建期实现、临时文件或主 bitcode 文件名：

- CMake 由 target graph、`compile_commands.json` 和生成器链接命令恢复信息；
- wrapper 由真实执行的 Clang/archiver/安装/文件操作命令恢复信息。

“发布树”指安装完成后的 LLVM IR 目录；构建目录的日志、sidecar、compile database、link command 和绝对路径不是发布接口。

## 2. 目录与伴随工件

按 native 安装类别，LLVM IR 位于以下目录：

```text
<libdir>/llvmir/                  库（含 shared/module/static）
<libdir>/llvmir-bin/              bin executable
<libdir>/llvmir-sbin/             sbin executable
<libdir>/llvmir-libexec/          libexec executable
```

每个逻辑产物 `N` 可有以下伴随文件：

```text
<ir-dir>/<main-bitcode>           必需；见下文的入口差异
<ir-dir>/N_cmd                    可选；executable/shared/module 的重链脚本
<ir-dir>/N_verscript              可选；`_cmd` 实际使用的版本脚本副本
<ir-dir>/asm/<id>-<name>          可选；native fallback object/archive
```

`N` 对静态库是 archive 文件名；对 shared/module 是其重链输出名（通常为 ELF SONAME）；对 executable 是安装文件名。若脚本存在，`_cmd`、`_verscript` 和 `asm/` 必须和对应主 bitcode 同目录。

### 2.1 主 bitcode 名称不是公共固定格式

当前主 bitcode 的命名是入口特定行为，消费者不得跨入口硬编码 `.bc`：

| 产物 | CMakeEmitLLVMIR | clang-wrap |
|---|---|---|
| executable/shared/module | `N.bc` | `N` |
| static library | `N`（LLVM archive） | `N`（LLVM archive） |

安装器、入口专用元数据或与主工件配套的 `_cmd` 应负责发现主 bitcode。协议只要求它在相应 IR 目录中且可被对应 `_cmd` 用 `./` 相对路径使用。

## 3. 消费者可依赖的共同事实

1. 静态库是 `llvm-ar rcsD` 创建的 LLVM archive，而不是预先合并的单一 module。
2. 若存在 `_cmd`，它是 mode `0755` 的 Bash 脚本，使用 Bash array quoting、`set -e`、`mkdir -p output` 与 `--output=output/N`。其命令包含 `-fuse-ld=lld`、`-x ir`，并记录 `# Clang version: ...`。
3. `_cmd` 必须能从自身所在目录执行；其随包 native fallback 和版本脚本必须以 `./asm/...`、`./N_verscript` 等相对路径引用。
4. `asm/<id>-<name>` 的 `id` 为原 native 输入**绝对路径** SHA-256 的前 12 位；`name` 是 basename。这是两个入口已经靠近的命名约定。
5. 已分类的编译期参数不会作为重链依赖保留：包括 include/macro/PCH/resource-dir、`-isysroot`、`-Xclang`、`-Xpreprocessor`、prefix-map 和 `-ffat-lto-objects` 等。后者只控制原生编译产物的 LTO 对象形态；`_cmd` 重链的是已生成的 bitcode，必须移除该选项。
6. 已分类的构建期链接搜索信息不会进入安装脚本：原始 `-L`、dependency-file、`rpath-link` 和非 `$ORIGIN` rpath 被删除；可解析共享库可被规范化为 `-l:SONAME`。group/whole-archive 等状态参数应保留。
7. 版本脚本若被支持并复制，脚本引用的是同目录 `-Wl,--version-script=./N_verscript`，而不是构建树原路径。

第 5–7 项仅涵盖已实现的参数分类；未知 linker 参数并不自动获得可移植保证。

## 4. 禁止出现在发布树或发布脚本中的状态

发布树不应包含或引用：

- `N_missing`、`N_log`、CMake `link.txt`、`compile_commands.json`、response file；
- 原 build tree、临时目录或 `DESTDIR` 路径；
- 仅存在于原构建目录的 native object/archive/linker script；
- 需要先运行另一个 `_cmd` 才能发现的同包输入。

`_missing` 是 wrapper 的安装转换 sidecar：静态 archive 安装时其 native member 写回 LLVM archive；executable/shared 安装时其成员复制至 `asm/` 并改写 `_cmd`。CMake 在构建期完成已知 native fallback 的复制，不应安装该 sidecar。无论来源，安装完成后都不得发布 `_missing`。

## 5. 不可重定位链接路径的共同安全边界

对 `--sysroot`、`-T`/`--script`，以及通过 `-Wl,` 或 `-Xlinker` 传递的绝对链接输入，当前实现不能可靠重定位。两条入口的共同保证是：**不得生成包含原路径的可安装 `_cmd`。**

- CMake 在 link-script custom command 中报 `FATAL_ERROR`；native target 可能已完成，但 LLVM IR target 失败且无 `_cmd`。
- clang-wrap 在 native link 后生成 `_cmd` 时返回错误，清理刚生成的主 IR/sidecar 并使当前 wrapper 调用失败。

这不是可依赖的“拒绝脚本”接口，也不是静默忽略。若将来实现安全复制或重写，须增加测试后再扩展本协议。

## 6. 明确不属于最小公共协议的差异

- CMake 的 `N.version.txt`、`N.soversion.txt` 来自 target `VERSION`/`SOVERSION`，wrapper 不生成；消费者不能要求它们存在。
- CMake 只处理受支持生成器中的 C/C++ target，并只安装已跟踪 `install(TARGETS)` 的工件；wrapper 覆盖的范围取决于实际被包装的命令和安装工具。
- 目标/源哈希命名、wrapper 的 `LLVM_IR_DIR` 镜像布局、`_log` 内容和构建时 `_missing` 生命周期均为内部实现细节。
- CMake 的编译命令选择可精确到 target/source；wrapper 对直接 driver 调用覆盖更广。这些差异不能仅通过比较最终文件名消除。

## 7. 验收与维护

当前已有各自入口的单独测试，但尚没有证明完全等价的双入口测试。在声称更高程度对齐前，应以同一 fixture 检查：

1. 发布树无 `*_missing`、`*_log`、build-tree 或 `DESTDIR` 泄漏；
2. `_cmd` 在其目录中可执行，且所有本包输入均可相对定位；
3. archive members、SONAME、`DT_NEEDED`、导出符号和运行结果符合 fixture 预期；
4. mixed archive、whole archive、外部 native 输入、版本脚本与非标准 runtime destination；
5. 不可重定位链接路径明确失败且不泄漏原路径。

修改任一入口的安装布局、主工件发现方式、`_cmd` 参数过滤、native fallback 命名或辅助文件生命周期时，必须同步更新本文件和 `CLANG_WRAP_COMPARISON.md`。前者只记录消费者可依赖的边界，后者记录实现差异及后续靠近方向。
