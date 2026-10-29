#!/bin/bash
# ============================================================================
# CMakeEmitLLVMIR 项目测试脚本
# ============================================================================
# 用于测试 CMakeEmitLLVMIR 模块对各种项目的支持
#
# 使用方法:
#   ./testdeb.sh              # 测试所有项目
#   ./testdeb.sh <project>    # 测试单个项目
#   ./testdeb.sh --clean      # 清理测试目录
#   ./testdeb.sh --help       # 显示帮助信息
#
# 支持的项目:
#   abseil, brotli, c-ares, libjpeg-turbo, libpng, leveldb, spdlog, 
#   googletest, cpuinfo, glfw, yaml-cpp, zlib, libxml2, curl, rapidjson,
#   armadillo, libwebp, tidy-html5, highway, flac, libuv, lz4, snappy,
#   json-c, libogg, libzstd
# ============================================================================

set -e

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# 允许通过环境变量覆盖默认路径
PROJECTS_DIR="${PROJECTS_DIR:-${SCRIPT_DIR}}"
CMAKE_EMIT_LLVMIR_MODULE="${PROJECTS_DIR}/cmake/CMakeEmitLLVMIR.cmake"
TEST_BASE_DIR="${TEST_BASE_DIR:-${SCRIPT_DIR}/testdeb}"
SOURCES_BASE_DIR="${TEST_BASE_DIR}/source"
BUILD_BASE_DIR="${TEST_BASE_DIR}/build"
INSTALL_BASE_DIR="${TEST_BASE_DIR}/install"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}============================================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}============================================================${NC}\n"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}ℹ️  $1${NC}"
}

verify_llvm_archives() {
    local archive
    for archive in "$@"; do
        if [ ! -f "$archive" ]; then
            print_error "未找到静态库 LLVM archive：$archive"
            return 1
        fi
        llvm-ar-22 t "$archive" > /dev/null
        print_info "$(basename "$archive") archive 验证通过"
    done
}

# 清理函数
cleanup() {
    print_info "清理测试目录..."
    rm -rf "${TEST_BASE_DIR}"
}

# 准备环境
prepare() {
    print_header "准备测试环境"

    # 检查编译器
    if ! command -v clang-22 &> /dev/null; then
        print_error "clang-22 未安装"
        exit 1
    fi

    if ! command -v llvm-link-22 &> /dev/null; then
        print_error "llvm-link-22 未安装"
        exit 1
    fi

    # 创建测试目录
    mkdir -p "${SOURCES_BASE_DIR}" "${BUILD_BASE_DIR}" "${INSTALL_BASE_DIR}"

    print_success "环境检查通过"
    print_info "clang: $(clang-22 --version | head -1)"
    print_info "llvm-link: $(llvm-link-22 --version | head -1)"
}

# 获取 Debian 源码包，并返回其中唯一的解压源码目录。
prepare_source() {
    local source_name="$1"
    local package_name="$2"
    local source_root="${SOURCES_BASE_DIR}/${source_name}"
    local source_dirs=()

    if [ ! -d "$source_root" ]; then
        print_info "下载 $source_name 源码..." >&2
        mkdir -p "$source_root"
        (
            cd "$source_root"
            apt-get source "$package_name"
        ) >&2
    fi

    mapfile -t source_dirs < <(find "$source_root" -mindepth 1 -maxdepth 1 -type d -print | sort)
    if [ "${#source_dirs[@]}" -ne 1 ]; then
        print_error "未找到唯一的源码目录：$source_root" >&2
        return 1
    fi

    printf '%s\n' "${source_dirs[0]}"
}

# 测试链接脚本的辅助函数
# 参数：$1 = install 目录路径
#       $2 = 排除模式（可选，支持通配符，如 "cwebp*" 或 "*_test*"）
# 用法示例：
#   test_link_scripts "$install_dir"              # 测试所有链接脚本
#   test_link_scripts "$install_dir" "cwebp*"     # 排除 cwebp 开头的脚本
#   test_link_scripts "$install_dir" "*_test* *demo*"  # 排除多个模式（空格分隔）
test_link_scripts() {
    local install_dir="$1"
    local exclude_patterns="$2"
    local total_success=0
    local total_fail=0
    local total_skipped=0

    # Runtime packages expose a library through its SONAME, not necessarily
    # through the concrete package-version filename used by the native build.
    # Verify every generated -l: shared-library operand against the ELF
    # DT_SONAME, without making assumptions about the SONAME version format.
    verify_shared_library_arguments() {
        local cmd="$1"
        local library_name library_path actual_soname search_dir
        local -a search_dirs library_names

        if grep -Eq "'/(usr|lib)/[^']*/lib[^']*\\.so(\\.[^']+)?'" "$cmd"; then
            print_info "⚠️  $cmd 包含未规范化的绝对共享库路径"
            return 1
        fi

        mapfile -t search_dirs < <(sed -n "s|^[[:space:]]*'-L\\(.*\\)'$|\\1|p" "$cmd")
        mapfile -t library_names < <(sed -n "s|^[[:space:]]*'-l:\\([^']*\\)'$|\\1|p" "$cmd")
        for library_name in "${library_names[@]}"; do
            # -l: can address archives as well; only shared libraries carry
            # the DT_SONAME contract under test.
            if [[ "$library_name" != *.so && "$library_name" != *.so.* ]]; then
                continue
            fi

            library_path=""
            for search_dir in "${search_dirs[@]}"; do
                if [[ "$search_dir" != /* ]]; then
                    search_dir="$(pwd)/$search_dir"
                fi
                if [ -e "$search_dir/$library_name" ]; then
                    library_path="$search_dir/$library_name"
                    break
                fi
            done
            if [ -z "$library_path" ] && command -v ldconfig >/dev/null 2>&1; then
                library_path=$(ldconfig -p 2>/dev/null |
                    awk -v name="$library_name" '$1 == name { print $NF; exit }')
            fi
            if [ -z "$library_path" ] || [ ! -e "$library_path" ]; then
                print_info "⚠️  $cmd 无法定位 -l:$library_name 对应的共享库"
                return 1
            fi

            actual_soname=$(readelf -d "$library_path" 2>/dev/null |
                sed -n 's/.*Library soname: \[\([^]]*\)\].*/\1/p' | head -1)
            if [ "$actual_soname" != "$library_name" ]; then
                print_info "⚠️  $cmd 的 -l:$library_name 不等于 $library_path 的 SONAME：$actual_soname"
                return 1
            fi
        done
    }
    
    # 查找所有 LLVM IR 目录（包括插件的嵌套目录和 llvmir-bin 等）。
    while IFS= read -r llvmir_dir; do
        
        cd "$llvmir_dir"
        if ls *_cmd 1> /dev/null 2>&1; then
            print_info "测试 $llvmir_dir 中的链接脚本..."
            for cmd in *_cmd; do
                # 检查是否需要排除
                local should_skip=0
                if [ -n "$exclude_patterns" ]; then
                    for pattern in $exclude_patterns; do
                        if [[ "$cmd" == $pattern ]]; then
                            should_skip=1
                            break
                        fi
                    done
                fi
                
                if [ $should_skip -eq 1 ]; then
                    total_skipped=$((total_skipped + 1))
                    print_info "⏭️  跳过 $cmd（匹配排除模式）"
                    continue
                fi
                
                print_info "运行 $cmd..."
		if ! verify_shared_library_arguments "$cmd"; then
                    total_fail=$((total_fail + 1))
                    continue
                fi
		set +e
                if bash "$cmd" >/dev/null 2>&1; then
                    if [[ "$cmd" == *.so*_cmd ]]; then
                        local soname="${cmd%_cmd}"
                        local relinked_library="output/$soname"
                        if [ ! -f "$relinked_library" ] || [ -L "$relinked_library" ]; then
                            # A versioned SONAME may make the actual output
                            # name longer than the link-script basename
                            # (for example libcpuinfo.so.0 vs libcpuinfo.so_cmd).
                            local versioned_library
                            versioned_library=$(find output -maxdepth 1 -type f -name "${soname}.*" -print -quit)
                            if [ -n "$versioned_library" ]; then
                                relinked_library="$versioned_library"
                            fi
                        fi
                        if [ ! -f "$relinked_library" ] || [ -L "$relinked_library" ]; then
                            total_fail=$((total_fail + 1))
                            print_info "⚠️  $cmd 未生成 SONAME 对应的动态库：$relinked_library"
                        else
                            local actual_soname
                            actual_soname=$(readelf -d "$relinked_library" 2>/dev/null |
                                sed -n 's/.*Library soname: \[\([^]]*\)\].*/\1/p' | head -1)
                            local relinked_basename="$(basename "$relinked_library")"
                            if [ -z "$actual_soname" ]; then
                            total_success=$((total_success + 1))
                            print_info "✅ $cmd 未声明 SONAME"
                            elif [ "$actual_soname" != "$relinked_basename" ]; then
                            total_fail=$((total_fail + 1))
                            print_info "⚠️  $cmd 的 SONAME 与文件名不一致"
                            else
                            total_success=$((total_success + 1))
                            print_info "✅ $cmd 的 SONAME 为 $actual_soname"
                            fi
                        fi
                    else
                        total_success=$((total_success + 1))
                    fi
                else
                    total_fail=$((total_fail + 1))
                    print_info "⚠️  $cmd 失败"
                fi
		set -e
            done
        fi
    done < <(find "$install_dir/lib" -type d -name 'llvmir*' -print | sort)
    
    if [ $total_skipped -gt 0 ]; then
        print_info "链接脚本结果：$total_success 成功，$total_fail 失败，$total_skipped 跳过"
    else
        print_info "链接脚本结果：$total_success 成功，$total_fail 失败"
    fi
    
    # 显示 output 目录内容
    while IFS= read -r llvmir_dir; do
        if [ -d "$llvmir_dir/output" ]; then
            print_info "$llvmir_dir/output 内容:"
            ls -la "$llvmir_dir/output/" 2>/dev/null | head -5
            file "$llvmir_dir/output/"* 2>/dev/null | head -5
        fi
    done < <(find "$install_dir/lib" -type d -name 'llvmir*' -print | sort)
    
    if [ $total_fail -eq 0 ]; then
        print_success "链接脚本验证通过"
        return 0
    else
        print_error "部分链接脚本失败"
        return 1
    fi
}

# 测试 abseil 项目（同时构建静态库和动态库）
test_abseil() {
    local project_name="abseil"
    local project_dir="$(prepare_source "abseil" "abseil")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (Google 基础库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DABSL_BUILD_TESTING=ON \
        -DABSL_ENABLE_INSTALL=ON \
        -DABSL_USE_EXTERNAL_GOOGLETEST=ON \
        -DABSL_BUILD_TEST_HELPERS=ON \
        -DBUILD_SHARED_LIBS=ON \
        -DABSL_BUILD_MONOLITHIC_SHARED_LIBS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10

        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 brotli 项目（同时构建静态库和动态库）
test_brotli() {
    local project_name="brotli"
    local project_dir="$(prepare_source "brotli" "brotli")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (压缩库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBROTLI_BUILD_TESTS=ON \
        -DBROTLI_BUILD_TOOLS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
            
            print_info "测试动态库链接脚本..."
            test_link_scripts "$install_dir"
        fi
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 c-ares 项目（同时构建静态库和动态库）
test_c_ares() {
    local project_name="c-ares"
    local project_dir="$(prepare_source "c-ares" "c-ares")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (DNS 解析库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DCARES_STATIC=ON \
        -DCARES_STATIC_PIC=ON \
        -DCARES_SHARED=ON \
        -DCARES_SYMBOL_HIDING=ON \
        -DCARES_BUILD_TESTS=ON \
        -DCARES_COVERAGE=ON \
        -DCARES_BUILD_TOOLS=ON \
        -DCMAKE_C_FLAGS="-D_GNU_SOURCE" \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
            
            print_info "测试动态库链接脚本..."
            test_link_scripts "$install_dir"
        fi
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libjpeg-turbo 项目（同时构建静态库和动态库）
test_libjpeg_turbo() {
    local project_name="libjpeg-turbo"
    local project_dir="$(prepare_source "libjpeg-turbo" "libjpeg-turbo")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (图像编解码库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库，并注入 -ffat-lto-objects）..."
    LDFLAGS="-ffat-lto-objects" CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DENABLE_STATIC=ON \
        -DENABLE_SHARED=ON \
        -DWITH_TURBOJPEG=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    if ! rg -l -g link.txt --fixed-strings -- "-ffat-lto-objects" "$build_dir/CMakeFiles" > /dev/null; then
        print_error "原生链接命令未包含 LDFLAGS 中的 -ffat-lto-objects"
        return 1
    fi
    print_success "原生链接命令包含 -ffat-lto-objects"

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"

            local cmd_files=()
            mapfile -t cmd_files < <(find "$install_dir/lib/llvmir" "$install_dir/lib/llvmir-bin" \
                -type f -name '*_cmd' -print 2>/dev/null)
            if [ "${#cmd_files[@]}" -eq 0 ]; then
                print_error "未找到用于检查 -ffat-lto-objects 的 _cmd 文件"
                return 1
            fi
            if rg -n --fixed-strings -- "-ffat-lto-objects" "${cmd_files[@]}"; then
                print_error "生成的 _cmd 保留了 -ffat-lto-objects"
                return 1
            fi
            print_success "生成的 _cmd 已过滤 -ffat-lto-objects"
            
            print_info "测试动态库链接脚本..."
            test_link_scripts "$install_dir"
        fi
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libpng 项目（同时构建静态库和动态库）
test_libpng() {
    local project_name="libpng"
    local project_dir="$(prepare_source "libpng" "libpng1.6")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (PNG 图像处理库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
            
            print_info "测试动态库链接脚本..."
            test_link_scripts "$install_dir"
        fi
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 leveldb 项目（静态库）
test_leveldb_static() {
    local project_name="leveldb-static"
    local project_dir="$(prepare_source "leveldb" "leveldb")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 leveldb (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        -DLEVELDB_BUILD_TESTS=ON \
        -DLEVELDB_BUILD_BENCHMARKS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.a | head -5
        local archive
        for archive in "$install_dir/lib/llvmir/"*.a; do
            print_info "检查 $(basename "$archive")..."
            llvm-ar-22 t "$archive" > /dev/null
            llvm-ar-22 t "$archive" | head -10
        done
        print_success "静态库 LLVM archive 验证通过"
    else
        print_error "未找到静态库 LLVM archive"
        return 1
    fi

    print_success "leveldb-static 测试通过"
}

# 测试 leveldb 项目（动态库）
test_leveldb_shared() {
    local project_name="leveldb-shared"
    local project_dir="$(prepare_source "leveldb" "leveldb")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 leveldb (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DLEVELDB_BUILD_TESTS=ON \
        -DLEVELDB_BUILD_BENCHMARKS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"
        
        # 测试动态库链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "leveldb-shared 测试通过"
}

# 测试 spdlog 项目（静态库）
test_spdlog_static() {
    local project_name="spdlog-static"
    local project_dir="$(prepare_source "spdlog" "spdlog")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 spdlog (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DSPDLOG_BUILD_TESTS=ON \
        -DSPDLOG_BUILD_EXAMPLE=ON \
        -DSPDLOG_BUILD_BENCH=ON \
        -DSPDLOG_SANITIZE_ADDRESS=ON \
        -DSPDLOG_SANITIZE_THREAD=OFF \
        -DSPDLOG_BUILD_SHARED=OFF \
        -DBUILD_SHARED_LIBS=OFF \
        -DSPDLOG_FMT_EXTERNAL=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libspdlog.a"

    print_success "spdlog-static 测试通过"
}

# 测试 spdlog 项目（动态库）
test_spdlog_shared() {
    local project_name="spdlog-shared"
    local project_dir="$(prepare_source "spdlog" "spdlog")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 spdlog (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DSPDLOG_BUILD_TESTS=ON \
        -DSPDLOG_BUILD_EXAMPLE=ON \
        -DSPDLOG_BUILD_BENCH=ON \
        -DSPDLOG_SANITIZE_ADDRESS=OFF \
        -DSPDLOG_SANITIZE_THREAD=OFF \
        -DSPDLOG_BUILD_SHARED=ON \
        -DBUILD_SHARED_LIBS=ON \
        -DSPDLOG_FMT_EXTERNAL=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "spdlog-shared 测试通过"
}

# 测试 googletest 项目（静态库）
test_googletest_static() {
    local project_name="googletest-static"
    local project_dir="$(prepare_source "googletest" "googletest")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 googletest (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_GMOCK=ON \
        -DBUILD_GTEST=ON \
        -DBUILD_SHARED_LIBS=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/"*.a

    print_success "googletest-static 测试通过"
}

# 测试 googletest 项目（动态库）
test_googletest_shared() {
    local project_name="googletest-shared"
    local project_dir="$(prepare_source "googletest" "googletest")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 googletest (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_GMOCK=ON \
        -DBUILD_GTEST=ON \
        -DBUILD_SHARED_LIBS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "googletest-shared 测试通过"
}

# 测试 cpuinfo 项目（静态库）
patch_cpuinfo_cxx_standard() {
    local project_dir="$1"
    local cmake_file="${project_dir}/CMakeLists.txt"

    if grep -Fq 'CXX_STANDARD 14' "$cmake_file"; then
        print_info "将 cpuinfo 目标 C++ 标准从 C++14 调整为 C++17..."
        sed -i 's/CXX_STANDARD 14/CXX_STANDARD 17/' "$cmake_file"
    fi

    if ! grep -Fq 'CXX_STANDARD 17' "$cmake_file"; then
        print_error "未找到 cpuinfo 的 CXX_STANDARD 设置：$cmake_file"
        return 1
    fi
}

test_cpuinfo_static() {
    local project_name="cpuinfo-static"
    local project_dir="$(prepare_source "cpuinfo" "cpuinfo")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 cpuinfo (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    patch_cpuinfo_cxx_standard "$project_dir"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DCPUINFO_LIBRARY_TYPE=static \
        -DGOOGLETEST_SOURCE_DIR=/usr/src/googletest/ \
        -DCPUINFO_BUILD_BENCHMARKS=OFF \
        -DUSE_SYSTEM_LIBS=ON \
	-DCMAKE_CXX_FLAGS=-std=c++17 \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libcpuinfo.a"

    print_success "cpuinfo-static 测试通过"
}

# 测试 cpuinfo 项目（动态库）
test_cpuinfo_shared() {
    local project_name="cpuinfo-shared"
    local project_dir="$(prepare_source "cpuinfo" "cpuinfo")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 cpuinfo (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    patch_cpuinfo_cxx_standard "$project_dir"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DCPUINFO_LIBRARY_TYPE=shared \
        -DGOOGLETEST_SOURCE_DIR=/usr/src/googletest/ \
        -DCPUINFO_BUILD_BENCHMARKS=OFF \
        -DUSE_SYSTEM_LIBS=ON \
	-DCMAKE_CXX_FLAGS=-std=c++17 \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "cpuinfo-shared 测试通过"
}

# 测试 glfw 项目（静态库）
test_glfw_static() {
    local project_name="glfw-static"
    local project_dir="$(prepare_source "glfw" "glfw3")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 glfw (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DGLFW_LIBRARY_TYPE=STATIC \
        -DGLFW_BUILD_WAYLAND=ON \
        -DGLFW_BUILD_X11=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libglfw3.a"

    print_success "glfw-static 测试通过"
}

# 测试 glfw 项目（动态库）
test_glfw_shared() {
    local project_name="glfw-shared"
    local project_dir="$(prepare_source "glfw" "glfw3")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 glfw (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DGLFW_LIBRARY_TYPE=SHARED \
        -DGLFW_BUILD_WAYLAND=ON \
        -DGLFW_BUILD_X11=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "glfw-shared 测试通过"
}

patch_yaml_cpp_cstdint() {
    local project_dir="$1"
    local source_file="${project_dir}/src/emitterutils.cpp"

    if ! grep -Fq '#include <cstdint>' "$source_file"; then
        print_info "为 yaml-cpp 的 emitterutils.cpp 添加 <cstdint>..."
        sed -i '/^#include <algorithm>$/a #include <cstdint>' "$source_file"
    fi
}

# 测试 yaml-cpp 项目（静态库）
test_yaml_cpp_static() {
    local project_name="yaml-cpp-static"
    local project_dir="$(prepare_source "yaml-cpp" "yaml-cpp")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 yaml-cpp (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    patch_yaml_cpp_cstdint "$project_dir"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DYAML_BUILD_SHARED_LIBS=OFF \
        -DYAML_CPP_BUILD_TESTS=ON \
        -DYAML_USE_SYSTEM_GTEST=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libyaml-cpp.a"

    print_success "yaml-cpp-static 测试通过"
}

# 测试 yaml-cpp 项目（动态库）
test_yaml_cpp_shared() {
    local project_name="yaml-cpp-shared"
    local project_dir="$(prepare_source "yaml-cpp" "yaml-cpp")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 yaml-cpp (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    patch_yaml_cpp_cstdint "$project_dir"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_CXX_STANDARD=17 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DYAML_BUILD_SHARED_LIBS=ON \
        -DYAML_CPP_BUILD_TESTS=ON \
        -DYAML_USE_SYSTEM_GTEST=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "yaml-cpp-shared 测试通过"
}

patch_zlib_cmake() {
    local project_dir="$1"
    local cmake_file="${project_dir}/CMakeLists.txt"

    if grep -Fq 'win32/zlib1.rc # If present will override custom build rule below.' "$cmake_file"; then
        print_info "将 zlib 的 ZLIB_DLL_SRCS 设置为空..."
        sed -i '/^[[:space:]]*set(ZLIB_DLL_SRCS[[:space:]]*$/,/^[[:space:]]*)[[:space:]]*$/c\    set(ZLIB_DLL_SRCS)' "$cmake_file"
    fi

}

# 测试 zlib 项目（同时构建静态库和动态库）
test_zlib() {
    local project_name="zlib"
    local project_dir="$(prepare_source "zlib" "zlib")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (压缩库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    patch_zlib_cmake "$project_dir"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DZLIB_BUILD_SHARED=ON \
        -DZLIB_BUILD_STATIC=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10

        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi

        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
        fi

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libxml2 项目（静态库）
test_libxml2_static() {
    local project_name="libxml2-static"
    local project_dir="$(prepare_source "libxml2" "libxml2")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 libxml2 (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        -DLIBXML2_WITH_PYTHON=OFF \
        -DLIBXML2_WITH_PROGRAMS=ON \
        -DLIBXML2_WITH_READLINE=ON \
        -DLIBXML2_WITH_ICU=ON \
        -DLIBXML2_WITH_ICONV=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j8

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libxml2.a"

    print_success "libxml2-static 测试通过"
}

# 测试 libxml2 项目（动态库）
test_libxml2_shared() {
    local project_name="libxml2-shared"
    local project_dir="$(prepare_source "libxml2" "libxml2")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 libxml2 (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DLIBXML2_WITH_PYTHON=ON \
        -DLIBXML2_WITH_PROGRAMS=ON \
        -DLIBXML2_WITH_READLINE=ON \
        -DLIBXML2_WITH_ICU=ON \
        -DLIBXML2_WITH_ICONV=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "libxml2-shared 测试通过"
}

# 测试 curl 项目（同时构建静态库和动态库）
test_curl() {
    local project_name="curl"
    local project_dir="$(prepare_source "curl" "curl")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (HTTP 客户端库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DBUILD_STATIC_LIBS=ON \
        -DBUILD_CURL_EXE=ON \
        -DBUILD_TESTING=ON \
        -DCURL_DISABLE_DOCS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个库位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10

        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi

        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
        fi

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到库位码文件"
        return 1
    fi

    # 验证可执行文件位码（如果启用）
    if ls "$install_dir/lib/llvmir-bin/"*_cmd 1> /dev/null 2>&1; then
        print_info "验证可执行文件链接脚本..."
        local exe_cmd_count=$(ls "$install_dir/lib/llvmir-bin/"*_cmd 2>/dev/null | wc -l)
        print_success "找到 $exe_cmd_count 个可执行文件链接脚本"
        ls "$install_dir/lib/llvmir-bin/"*_cmd 2>/dev/null | head -5
    fi

    print_success "$project_name 测试通过"
}

# 测试 rapidjson 项目（头文件库）
test_rapidjson() {
    local project_name="rapidjson"
    local project_dir="$(prepare_source "rapidjson" "rapidjson")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (JSON 解析库，头文件库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DRAPIDJSON_BUILD_DOC=OFF \
        -DRAPIDJSON_BUILD_EXAMPLES=OFF \
        -DRAPIDJSON_BUILD_TESTS=OFF \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    # rapidjson 是头文件库，可能没有位码文件
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"
    else
        print_info "rapidjson 是头文件库，没有编译的位码文件"
        print_success "头文件库验证通过"
    fi

    print_success "$project_name 测试通过"
}

# 测试 armadillo 项目（头文件库 + 包装器库）
test_armadillo_shared() {
    local project_name="armadillo-shared"
    local project_dir="$(prepare_source "armadillo" "armadillo")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (线性代数库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DHEADER_ONLY=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_info "armadillo 是头文件库，没有编译的位码文件"
        print_success "头文件库验证通过"
    fi

    print_success "$project_name 测试通过"
}

# 测试 armadillo 项目（静态库）
test_armadillo_static() {
    local project_name="armadillo-static"
    local project_dir="$(prepare_source "armadillo" "armadillo")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DSTATIC_LIB=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_info "armadillo 是头文件库，没有编译的位码文件"
        print_success "头文件库验证通过"
    fi

    print_success "$project_name 测试通过"
}

# 测试 libwebp 项目（动态库）
test_libwebp_shared() {
    local project_name="libwebp-shared"
    local project_dir="$(prepare_source "libwebp" "libwebp")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-22 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        local img2webp_script="$install_dir/lib/llvmir-bin/img2webp_cmd"
        print_info "验证外部动态库使用版本化路径..."
        if [ ! -f "$img2webp_script" ]; then
            print_error "未找到 img2webp 链接脚本：$img2webp_script"
            return 1
        fi
        if grep -Eq "'/usr/lib/[^']*/[^']+\.so'" "$img2webp_script"; then
            print_error "img2webp 链接脚本仍引用未版本化的系统动态库"
            return 1
        fi
        print_success "外部动态库版本化路径验证通过"

        # 测试动态库链接脚本
        if ls "$install_dir/lib/llvmir/"*_cmd 1> /dev/null 2>&1; then
            print_info "测试动态库链接脚本..."
            cd "$install_dir/lib/llvmir"
            for cmd in *_cmd; do
                print_info "运行 $cmd..."
                bash "$cmd" 2>&1 | tail -3
            done
            ls -la output/ 2>/dev/null | head -5
            file output/* 2>/dev/null | head -5
            print_success "链接脚本验证通过"
        fi

        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libwebp 项目（静态库）
test_libwebp_static() {
    local project_name="libwebp-static"
    local project_dir="$(prepare_source "libwebp" "libwebp")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/"*.a

    print_success "$project_name 测试通过"
}

# 测试 tidy-html5 项目（同时构建静态库和动态库）
test_tidy_html5() {
    local project_name="tidy-html5"
    local project_dir="$(prepare_source "tidy-html5" "tidy-html5")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (HTML 语法检查器，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIB=ON \
        -DSUPPORT_CONSOLE_APP=ON \
        -DBUILD_TAB2SPACE=ON \
        -DBUILD_SAMPLE_CODE=ON \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
        fi

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    # 验证可执行文件位码
    if ls "$install_dir/lib/llvmir-bin/"*_cmd 1> /dev/null 2>&1; then
        print_info "验证可执行文件链接脚本..."
        local exe_cmd_count=$(ls "$install_dir/lib/llvmir-bin/"*_cmd 2>/dev/null | wc -l)
        print_success "找到 $exe_cmd_count 个可执行文件链接脚本"
        ls "$install_dir/lib/llvmir-bin/"*_cmd 2>/dev/null | head -5
    fi

    print_success "$project_name 测试通过"
}

# 测试 highway 项目（静态库）
test_highway_static() {
    local project_name="highway-static"
    local project_dir="$(prepare_source "highway" "highway")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 highway (静态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-21 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-21 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        -DHWY_FORCE_STATIC_LIBS=ON \
        -DHWY_ENABLE_CONTRIB=ON \
        -DHWY_ENABLE_EXAMPLES=ON \
        -DHWY_ENABLE_TESTS=ON \
        -DHWY_ENABLE_INSTALL=ON \
        -DHWY_SYSTEM_GTEST=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/"*.a

    # 静态库的 bitcode 保存在 archive 成员中，不会生成顶层 *.bc 文件。
    # 某些静态库项目还会安装可执行程序，此时其 *_cmd 也必须验证。
    local executable_cmds=()
    if [ -d "$install_dir/lib/llvmir-bin" ]; then
        mapfile -t executable_cmds < <(
            find "$install_dir/lib/llvmir-bin" -maxdepth 1 -type f -name '*_cmd' -print | sort
        )
    fi
    if [ "${#executable_cmds[@]}" -gt 0 ]; then
        print_info "找到 ${#executable_cmds[@]} 个可执行文件链接脚本"
        printf '%s\n' "${executable_cmds[@]}" | head -5
    else
        print_info "未安装可执行文件链接脚本，跳过可执行文件 cmd 验证"
    fi

    # 同时验证库和可执行文件的所有已安装链接脚本；该函数会扫描
    # lib/llvmir、lib/llvmir-bin 以及其他 llvmir* 子目录。
    test_link_scripts "$install_dir"

    print_success "highway-static 测试通过"
}

# 测试 highway 项目（动态库）
test_highway_shared() {
    local project_name="highway-shared"
    local project_dir="$(prepare_source "highway" "highway")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 highway (动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-21 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-21 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DHWY_FORCE_STATIC_LIBS=OFF \
        -DHWY_ENABLE_CONTRIB=ON \
        -DHWY_ENABLE_EXAMPLES=ON \
        -DHWY_ENABLE_TESTS=ON \
        -DHWY_ENABLE_INSTALL=ON \
        -DHWY_SYSTEM_GTEST=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -5
        llvm-dis-21 "$install_dir/lib/llvmir/"*.bc -o - 2>/dev/null | head -10
        print_success "位码文件验证通过"

        # 测试所有链接脚本
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "highway-shared 测试通过"
}

# 测试 flac 项目（同时构建静态库和动态库）
test_flac() {
    local project_name="flac"
    local project_dir="$(prepare_source "flac" "flac")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (无损音频编解码库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DFLAC_ENABLE_STATIC=ON \
        -DINSTALL_MANPAGES=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库位码
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            verify_llvm_archives "$install_dir/lib/llvmir/"*.a
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
        fi
        
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libuv 项目（同时构建静态库和动态库）
test_libuv() {
    local project_name="libuv"
    local project_dir="$(prepare_source "libuv" "libuv1")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (异步 I/O 库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DLIBUV_BUILD_SHARED=ON \
        -DLIBUV_BUILD_STATIC=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        # 验证静态库 LLVM archive
        if ls "$install_dir/lib/llvmir/"*.a 1> /dev/null 2>&1; then
            print_info "验证静态库 LLVM archive..."
            llvm-ar-22 t "$install_dir/lib/llvmir/"*.a | head -10
            print_success "静态库 LLVM archive 验证通过"
        fi
        
        # 验证动态库位码
        if ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_info "验证动态库位码..."
            llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
            print_success "动态库位码验证通过"
        fi
        
        test_link_scripts "$install_dir"
    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 lz4 动态库
test_lz4_shared() {
    local project_name="lz4-shared"
    local source_dir="$(prepare_source "lz4" "lz4")"
    local project_dir="${source_dir}/build/cmake"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (LZ4 压缩库动态库)"

    if [ ! -f "$project_dir/CMakeLists.txt" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置动态库..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DLZ4_BUILD_CLI=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    local soname="liblz4.so.1"
    local shared_bc="$install_dir/lib/llvmir/$soname.bc"
    print_info "验证动态库位码..."
    if [ ! -f "$shared_bc" ]; then
        print_error "未找到动态库位码文件：$shared_bc"
        return 1
    fi
    llvm-dis-22 "$shared_bc" -o - 2>/dev/null | head -10
    print_success "动态库位码验证通过"

    test_link_scripts "$install_dir"

    print_success "$project_name 测试通过"
}

# 测试 lz4 静态库
test_lz4_static() {
    local project_name="lz4-static"
    local source_dir="$(prepare_source "lz4" "lz4")"
    local project_dir="${source_dir}/build/cmake"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (LZ4 压缩库静态库)"

    if [ ! -f "$project_dir/CMakeLists.txt" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（静态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        -DLZ4_BUILD_CLI=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/liblz4.a"
    print_success "$project_name 测试通过"
}

# 测试 snappy 项目（同时构建静态库和动态库）
test_snappy() {
    local project_name="snappy"
    local project_dir="$(prepare_source "snappy" "snappy")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (Snappy 压缩库，同时构建静态库 + 动态库)"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DSNAPPY_BUILD_TESTS=OFF \
        -DSNAPPY_BUILD_BENCHMARKS=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)
    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    local static_archive="$install_dir/lib/llvmir/libsnappy.a"
    local soname="libsnappy.so.1"
    local shared_bc="$install_dir/lib/llvmir/$soname.bc"
    print_info "验证位码文件..."
    if [ ! -f "$static_archive" ] || [ ! -f "$shared_bc" ]; then
        print_error "未找到预期的 snappy 静态库或动态库位码文件"
        return 1
    fi

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$static_archive"

    print_info "验证动态库位码..."
    llvm-dis-22 "$shared_bc" -o - 2>/dev/null | head -10
    print_success "动态库位码验证通过"

    test_link_scripts "$install_dir"

    print_success "$project_name 测试通过"
}

# 测试 json-c 项目（同时构建静态库和动态库）
test_json_c() {
    local project_name="json-c"
    local project_dir="$(prepare_source "json-c" "json-c")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (JSON 解析库，同时构建静态库 + 动态库)"

    if [ ! -d "$project_dir" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        local static_archive="$install_dir/lib/llvmir/libjson-c.a"
        local soname="libjson-c.so.5"
        local shared_bc="$install_dir/lib/llvmir/$soname.bc"
        if [ ! -f "$static_archive" ] || [ ! -f "$shared_bc" ]; then
            print_error "未找到预期的 json-c 静态库或动态库位码文件"
            return 1
        fi

        print_info "验证静态库 LLVM archive..."
        verify_llvm_archives "$static_archive"

        print_info "验证动态库位码..."
        llvm-dis-22 "$shared_bc" -o - 2>/dev/null | head -10
        print_success "动态库位码验证通过"

        local version_script="$install_dir/lib/llvmir/${soname}_verscript"
        local link_script="$install_dir/lib/llvmir/${soname}_cmd"
        print_info "验证版本脚本分发和链接脚本路径..."
        if [ ! -f "$version_script" ]; then
            print_error "未找到分发的 json-c 版本脚本：$version_script"
            return 1
        fi
        if ! grep -Fq -- "-Wl,--version-script=./${soname}_verscript" "$link_script"; then
            print_error "json-c 链接脚本未使用分发的相对版本脚本路径"
            return 1
        fi
        if grep -Fq -- "$project_dir" "$link_script"; then
            print_error "json-c 链接脚本仍引用源码树路径"
            return 1
        fi
        print_success "json-c 版本脚本分发验证通过"

        test_link_scripts "$install_dir"

    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libogg 动态库
test_libogg_shared() {
    local project_name="libogg-shared"
    local project_dir="$(prepare_source "libogg" "libogg")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (Ogg 容器格式动态库)"

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置动态库..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        if ! ls "$install_dir/lib/llvmir/"*.so*.bc 1> /dev/null 2>&1; then
            print_error "未找到动态库位码文件"
            return 1
        fi
        print_info "验证动态库位码..."
        llvm-dis-22 "$install_dir/lib/llvmir/"*.so*.bc -o - 2>/dev/null | head -10
        print_success "动态库位码验证通过"
        
        test_link_scripts "$install_dir"

    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 libogg 静态库
test_libogg_static() {
    local project_name="libogg-static"
    local project_dir="$(prepare_source "libogg" "libogg")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (Ogg 容器格式静态库)"
    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置静态库..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=OFF \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc)
    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证静态库 LLVM archive..."
    verify_llvm_archives "$install_dir/lib/llvmir/libogg.a"
}

test_libogg() {
    test_libogg_shared
    test_libogg_static
}

# 测试 libzstd 项目（同时构建静态库和动态库）
test_libzstd() {
    local project_name="libzstd"
    local source_dir="$(prepare_source "libzstd" "libzstd")"
    local project_dir="${source_dir}/build/cmake"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (Zstandard 压缩库，同时构建静态库 + 动态库)"

    if [ ! -f "$project_dir/CMakeLists.txt" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目（同时构建静态库和动态库）..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DBUILD_SHARED_LIBS=ON \
        -DZSTD_BUILD_SHARED=ON \
        -DZSTD_BUILD_STATIC=ON \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证位码文件..."
    if ls "$install_dir/lib/llvmir/"*.bc 1> /dev/null 2>&1; then
        local bc_count=$(ls "$install_dir/lib/llvmir/"*.bc | wc -l)
        print_success "找到 $bc_count 个位码文件"
        ls -lh "$install_dir/lib/llvmir/"*.bc | head -10
        
        local static_archive="$install_dir/lib/llvmir/libzstd.a"
        local soname="libzstd.so.1"
        local shared_bc="$install_dir/lib/llvmir/$soname.bc"
        if [ ! -f "$static_archive" ] || [ ! -f "$shared_bc" ]; then
            print_error "未找到预期的 libzstd 静态库或动态库位码文件"
            return 1
        fi

        print_info "验证静态库 LLVM archive..."
        verify_llvm_archives "$static_archive"

        print_info "验证动态库位码..."
        llvm-dis-22 "$shared_bc" -o - 2>/dev/null | head -10
        print_success "动态库位码验证通过"
        
        test_link_scripts "$install_dir"

    else
        print_error "未找到位码文件"
        return 1
    fi

    print_success "$project_name 测试通过"
}

# 测试 MariaDB 项目（CMake）
test_mariadb() {
    local project_name="mariadb"
    local project_dir="$(prepare_source "$project_name" "$project_name")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 MariaDB (CMake)"

    if [ ! -f "$project_dir/CMakeLists.txt" ]; then
        print_error "项目目录不存在：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目..."
    # Keep WSREP enabled so MariaDB leaves its exclusion-pattern variables
    # empty; the install() wrapper must preserve those empty PATTERN values.
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/home/syq/develop/llvm-project/build-rel/bin/clang \
        -DCMAKE_CXX_COMPILER=/home/syq/develop/llvm-project/build-rel/bin/clang++ \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        -DWITH_UNIT_TESTS=OFF \
        -DWITH_EMBEDDED_SERVER=ON \
        -DDISABLE_SHARED=OFF \
        -DWITH_WSREP=ON \
        -DWITH_PIC=ON \
	-DCMAKE_POSITION_INDEPENDENT_CODE=ON \
        -DGSSAPI_FLAVOR:STRING=MIT \
        "-DGSSAPI_LIBS:STRING=-L/usr/lib/x86_64-linux-gnu/mit-krb5 -Wl,--enable-new-dtags -Wl,-rpath -Wl,/usr/lib/x86_64-linux-gnu/mit-krb5 -lgssapi_krb5 -lkrb5 -lk5crypto -lcom_err" \
        -DPLUGIN_ROCKSDB=NO \
        -DPLUGIN_MROONGA=NO \
        -DPLUGIN_TOKUDB=NO \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_info "验证 MariaDB 动态库及位码文件..."
    local embedded_library
    embedded_library="$(find "$install_dir/lib" -maxdepth 1 -type f -name 'libmariadbd.so.*' -print -quit)"
    if [ -z "$embedded_library" ]; then
        print_error "未找到 embedded server 动态库：$install_dir/lib/libmariadbd.so.*"
        return 1
    fi
    local embedded_soname
    embedded_soname="$(basename "$embedded_library")"
    local embedded_bc="$install_dir/lib/llvmir/${embedded_soname}.bc"
    local embedded_cmd="$install_dir/lib/llvmir/${embedded_soname}_cmd"
    if [ ! -f "$embedded_bc" ] || [ ! -f "$embedded_cmd" ]; then
        print_error "未找到 embedded server LLVM IR 或链接脚本：${embedded_soname}.bc / ${embedded_soname}_cmd"
        return 1
    fi
    find "$install_dir/lib/llvmir" -maxdepth 1 -type f \
        \( -name "${embedded_soname}.bc" -o -name "${embedded_soname}_cmd" \) -print
    print_success "MariaDB embedded server 动态库和 LLVM IR 文件验证通过：$embedded_soname"

    print_info "验证 MariaDB LLVM IR 链接脚本..."
    test_link_scripts "$install_dir"
}

# 测试 ecbuild 项目（CMake 构建系统工具）
test_ecbuild() {
    local project_name="ecbuild"
    local project_dir="$(prepare_source "$project_name" "$project_name")"
    local build_dir="${BUILD_BASE_DIR}/${project_name}"
    local install_dir="${INSTALL_BASE_DIR}/${project_name}"

    print_header "测试 $project_name (CMake 构建系统工具)"

    if [ ! -f "$project_dir/CMakeLists.txt" ]; then
        print_error "项目目录不存在或缺少 CMakeLists.txt：$project_dir"
        return 1
    fi

    rm -rf "$build_dir" "$install_dir"
    mkdir -p "$build_dir"
    cd "$build_dir"

    print_info "配置项目..."
    CMAKE_EMIT_LLVMIR=1 cmake -G "Unix Makefiles" \
        -DCMAKE_C_COMPILER=/usr/bin/clang-22 \
        -DCMAKE_CXX_COMPILER=/usr/bin/clang++-22 \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES="${CMAKE_EMIT_LLVMIR_MODULE}" \
        -DCMAKE_INSTALL_PREFIX="$install_dir" \
        "$project_dir"

    print_info "构建项目..."
    cmake --build . -j$(nproc) -v

    print_info "安装项目..."
    cmake --install . --prefix "$install_dir"

    print_success "$project_name 测试通过"
}

# 显示帮助信息
show_help() {
    echo "CMakeEmitLLVMIR 项目测试脚本"
    echo ""
    echo "使用方法:"
    echo "  $0                      # 测试所有项目"
    echo "  $0 <project>            # 测试单个项目"
    echo "  $0 --clean              # 清理测试目录"
    echo "  $0 --help               # 显示帮助信息"
    echo ""
    echo "支持的项目:"
    echo "  abseil         - Google 基础库"
    echo "  brotli         - 压缩库"
    echo "  c-ares         - DNS 解析库"
    echo "  libjpeg-turbo  - 图像编解码库"
    echo "  libpng         - PNG 图像处理库"
    echo "  leveldb        - 数据库"
    echo "  spdlog         - 日志库"
    echo "  googletest     - 测试框架"
    echo "  cpuinfo        - CPU 信息检测库"
    echo "  glfw           - OpenGL 窗口库"
    echo "  yaml-cpp       - YAML 解析库"
    echo "  zlib           - 压缩库"
    echo "  libxml2        - XML 解析库"
    echo "  curl           - HTTP 客户端库"
    echo "  rapidjson      - JSON 解析库（头文件库）"
    echo "  armadillo      - 线性代数库"
    echo "  libwebp        - 图像编解码库"
    echo "  tidy-html5     - HTML 语法检查器"
    echo "  highway        - SIMD 库"
    echo "  flac           - 无损音频编解码库"
    echo "  libuv          - 异步 I/O 库"
    echo "  lz4            - 快速压缩库"
    echo "  snappy         - Google 压缩库"
    echo "  json-c         - JSON 解析库"
    echo "  libogg         - 测试 libogg 动态库和静态库"
    echo "  libzstd        - Zstandard 压缩库"
    echo "  mariadb        - MariaDB（CMake，含 embedded server 动态库）"
    echo "  ecbuild        - ecbuild（CMake 构建系统工具）"
    echo ""
    echo "注意：leveldb、spdlog、googletest、cpuinfo、glfw、yaml-cpp、libxml2, armadillo、libwebp 和 highway 支持静态库和动态库分别测试"
    echo "  leveldb-static   - 测试 leveldb 静态库"
    echo "  leveldb-shared   - 测试 leveldb 动态库"
    echo "  spdlog-static    - 测试 spdlog 静态库"
    echo "  spdlog-shared    - 测试 spdlog 动态库"
    echo "  googletest-static  - 测试 googletest 静态库"
    echo "  googletest-shared  - 测试 googletest 动态库"
    echo "  cpuinfo-static     - 测试 cpuinfo 静态库"
    echo "  cpuinfo-shared     - 测试 cpuinfo 动态库"
    echo "  glfw-static        - 测试 glfw 静态库"
    echo "  glfw-shared        - 测试 glfw 动态库"
    echo "  yaml-cpp-static    - 测试 yaml-cpp 静态库"
    echo "  yaml-cpp-shared    - 测试 yaml-cpp 动态库"
    echo "  libxml2-static     - 测试 libxml2 静态库"
    echo "  libxml2-shared     - 测试 libxml2 动态库"
    echo "  armadillo-shared     - 测试 armadillo 动态库"
    echo "  armadillo-static     - 测试 armadillo 静态库"
    echo "  libwebp-shared     - 测试 libwebp 动态库"
    echo "  libwebp-static     - 测试 libwebp 静态库"
    echo "  highway-shared     - 测试 highway 动态库"
    echo "  highway-static     - 测试 highway 静态库"
    echo "  libogg-shared      - 测试 libogg 动态库"
    echo "  libogg-static      - 测试 libogg 静态库"
    echo ""
    echo "注意：zlib 和 curl 支持同时构建静态库 + 动态库"
    echo "示例:"
    echo "  $0                      # 测试所有项目"
    echo "  $0 yaml-cpp             # 测试 yaml-cpp"
    echo "  $0 brotli c-ares        # 测试多个项目"
}

# 主函数
main() {
    if [ $# -eq 0 ]; then
        # 测试所有项目
        prepare

        test_abseil || print_error "abseil 失败"
        test_brotli || print_error "brotli 失败"
        test_c_ares || print_error "c-ares 失败"
        test_libjpeg_turbo || print_error "libjpeg-turbo 失败"
        test_libpng || print_error "libpng 失败"
        test_leveldb_static || print_error "leveldb-static 失败"
        test_leveldb_shared || print_error "leveldb-shared 失败"
        test_spdlog_static || print_error "spdlog-static 失败"
        test_spdlog_shared || print_error "spdlog-shared 失败"
        test_googletest_static || print_error "googletest-static 失败"
        test_googletest_shared || print_error "googletest-shared 失败"
        test_cpuinfo_static || print_error "cpuinfo-static 失败"
        test_cpuinfo_shared || print_error "cpuinfo-shared 失败"
        test_glfw_static || print_error "glfw-static 失败"
        test_glfw_shared || print_error "glfw-shared 失败"
        test_yaml_cpp_static || print_error "yaml-cpp-static 失败"
        test_yaml_cpp_shared || print_error "yaml-cpp-shared 失败"
        test_zlib || print_error "zlib 失败"
        test_libxml2_static || print_error "libxml2-static 失败"
        test_libxml2_shared || print_error "libxml2-shared 失败"
        test_curl || print_error "curl 失败"
        test_rapidjson || print_error "rapidjson 失败"
        test_armadillo_shared || print_error "armadillo-shared 失败"
        test_armadillo_static || print_error "armadillo-static 失败"
        #test_mariadb || print_error "mariadb 失败"

        test_libwebp_shared || print_error "libwebp-shared 失败"
        test_libwebp_static || print_error "libwebp-static 失败"
        test_tidy_html5 || print_error "tidy-html5 失败"

	# too slow
        test_highway_static || print_error "highway-static 失败"
        test_highway_shared || print_error "highway-shared 失败"

        print_header "测试完成"
        print_success "所有测试完成！"

    elif [ "$1" = "--clean" ]; then
        cleanup
        print_success "清理完成"

    elif [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
        show_help

    else
        # 测试指定项目
        prepare

        for project in "$@"; do
            case "$project" in
                abseil)
                    test_abseil
                    ;;
                brotli)
                    test_brotli
                    ;;
                c-ares)
                    test_c_ares
                    ;;
                libjpeg-turbo)
                    test_libjpeg_turbo
                    ;;
                libpng)
                    test_libpng
                    ;;
                leveldb)
                    test_leveldb_static
                    test_leveldb_shared
                    ;;
                leveldb-static)
                    test_leveldb_static
                    ;;
                leveldb-shared)
                    test_leveldb_shared
                    ;;
                spdlog)
                    test_spdlog_static
                    test_spdlog_shared
                    ;;
                spdlog-static)
                    test_spdlog_static
                    ;;
                spdlog-shared)
                    test_spdlog_shared
                    ;;
                googletest)
                    test_googletest_static
                    test_googletest_shared
                    ;;
                googletest-static)
                    test_googletest_static
                    ;;
                googletest-shared)
                    test_googletest_shared
                    ;;
                cpuinfo)
                    test_cpuinfo_static
                    test_cpuinfo_shared
                    ;;
                cpuinfo-static)
                    test_cpuinfo_static
                    ;;
                cpuinfo-shared)
                    test_cpuinfo_shared
                    ;;
                glfw)
                    test_glfw_static
                    test_glfw_shared
                    ;;
                glfw-static)
                    test_glfw_static
                    ;;
                glfw-shared)
                    test_glfw_shared
                    ;;
                yaml-cpp)
                    test_yaml_cpp_static
                    test_yaml_cpp_shared
                    ;;
                yaml-cpp-static)
                    test_yaml_cpp_static
                    ;;
                yaml-cpp-shared)
                    test_yaml_cpp_shared
                    ;;
                zlib)
                    test_zlib
                    ;;
                libxml2)
                    test_libxml2_static
                    test_libxml2_shared
                    ;;
                libxml2-static)
                    test_libxml2_static
                    ;;
                libxml2-shared)
                    test_libxml2_shared
                    ;;
                curl)
                    test_curl
                    ;;
                rapidjson)
                    test_rapidjson
                    ;;
                armadillo)
                    test_armadillo_shared
                    test_armadillo_static
                    ;;
                armadillo-shared)
                    test_armadillo_shared
                    ;;
                armadillo-static)
                    test_armadillo_static
                    ;;
                libwebp)
                    test_libwebp_shared
                    test_libwebp_static
                    ;;
                libwebp-shared)
                    test_libwebp_shared
                    ;;
                libwebp-static)
                    test_libwebp_static
                    ;;
                tidy-html5)
                    test_tidy_html5
                    ;;
                highway)
                    test_highway_static
                    test_highway_shared
                    ;;
                highway-static)
                    test_highway_static
                    ;;
                highway-shared)
                    test_highway_shared
                    ;;
                flac)
                    test_flac
                    ;;
                libuv)
                    test_libuv
                    ;;
                lz4)
                    test_lz4_static
                    test_lz4_shared
                    ;;
                lz4-static)
                    test_lz4_static
                    ;;
                lz4-shared)
                    test_lz4_shared
                    ;;
                snappy)
                    test_snappy
                    ;;
                json-c)
                    test_json_c
                    ;;
                libogg)
                    test_libogg
                    ;;
                libogg-shared)
                    test_libogg_shared
                    ;;
                libogg-static)
                    test_libogg_static
                    ;;
                libzstd)
                    test_libzstd
                    ;;
                mariadb)
                    test_mariadb
                    ;;
                ecbuild)
                    test_ecbuild
                    ;;
                *)
                    print_error "未知项目：$project"
                    show_help
                    exit 1
                    ;;
            esac
        done

        print_header "测试完成"
        print_success "指定项目测试完成！"
    fi
}

# 运行主函数
main "$@"
