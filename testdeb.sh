#!/bin/sh
set -e

# 代理设置
#export HTTPS_PROXY=10.200.7.1:8586

# 构建和安装 clang-wrap
build() {
	make clean-install
	make install
}

# 执行指定目录中的所有 _cmd 脚本。
# 注意：`bash *_cmd` 只会把第一个 _cmd 当作脚本执行，其余展开项会变成 $1/$2...
# 因此必须显式循环逐个执行。
run_llvmir_cmds() {
	dir=$1
	[ -d "$dir" ] || return 0
	(
		cd "$dir"
		for cmd in *_cmd; do
			[ -e "$cmd" ] || continue
			echo "Running: $dir/$cmd ..."
			bash "$cmd"
		done
	)
}

# 测试 libxml2
test_libxml2() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/libxml2
	(
		cd testdeb/libxml2
		rm -rf libxml2*
		apt-get source libxml2
		cd libxml2-2*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang-22 ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 pcre2
test_pcre2() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/pcre2
	(
		cd testdeb/pcre2
		rm -rf pcre2*
		#apt-get source pcre2
		dget -x -u http://mirror.iscas.ac.cn/debian/pool/main/p/pcre2/pcre2_10.48-3.1.dsc
		cd pcre2-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 flex
test_flex() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/flex
	(
		cd testdeb/flex
		rm -rf flex*
		apt-get source flex
		cd flex-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 bamf
test_bamf() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/bamf
	(
		cd testdeb/bamf
		rm -rf bamf*
		apt-get source bamf
		cd bamf-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
	)
}

test_hesiod() {
	pkg=hesiod
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang-22 CXX=clang++-22 ../configure && \
			CC=clang-22 CXX=clang++-22 make V=1 -j`nproc` && \
			make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_hwloc() {
	pkg=hwloc
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_identity4c() {
	pkg=identity4c
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
	)
}

test_paps() {
	pkg=paps
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_x264() {
	pkg=x264
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang ../configure --enable-shared && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_ffmpeg() {
	pkg=ffmpeg
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		# ffmpeg 需要禁用汇编和一些可能有问题的组件
		CC=clang CXX=clang++ ../configure --cc=clang --cxx=clang++ --enable-shared --disable-asm --disable-doc --disable-htmlpages --disable-manpages --disable-podpages --disable-txtpages && make V=1 -j`nproc` && make install V=1 DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_sqlite3() {
	pkg=sqlite3
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ../configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

test_libpcap() {
	pkg=libpcap
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ./configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
	)
}

test_gsl() {
	pkg=gsl
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		rm -rf ~/tmp/llvmir/`pwd`
		AUTOMAKE=automake ACLOCAL=aclocal dh_autoreconf
		CC=clang CXX=clang++ ./configure && make V=1 -j`nproc` && make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 libfyaml
test_libfyaml() {
	pkg=libfyaml
	export CLANG_WRAP_DEBUG=1
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		dh_autoreconf
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ ../configure
		make V=1 -j`nproc`
		make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 pixman（Meson 构建与安装）
test_pixman() {
	pkg=pixman
	export CLANG_WRAP_DEBUG=1
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		CC=clang CXX=clang++ meson setup --prefix=/usr . ..
		meson compile -C . -j`nproc` -v
		meson install -C . --destdir=`pwd`/install
		meson-install meson-info/intro-installed.json `pwd`/install
		triplet=`dpkg-architecture -qDEB_HOST_MULTIARCH`
		run_llvmir_cmds install/usr/lib/$triplet/llvmir
		run_llvmir_cmds install/usr/lib/llvmir-bin
	)
}

# 测试 xfsprogs（autotools）
test_xfsprogs() {
	pkg=xfsprogs
	export CLANG_WRAP_DEBUG=1
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	export CC=clang-22 CXX=clang++-22 AR=llvm-ar-22
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		# xfsprogs 的 configure 不支持 out-of-tree 构建。
		make distclean >/dev/null 2>&1 || true
		rm -rf ~/tmp/llvmir/`pwd`
		./configure && make V=1 -j`nproc` && \
			make install DESTDIR=`pwd`/install
		llvmir_install="$HOME/tmp/llvmir/`pwd`/install"
		if ! find "$llvmir_install" -name '*_cmd' -print -quit | grep -q .; then
			echo "ERROR: xfsprogs 未生成 LLVM IR 链接脚本" >&2
			return 1
		fi
		run_llvmir_cmds "$llvmir_install/usr/lib64"
		run_llvmir_cmds "$llvmir_install/usr/lib"
	)
}

# 测试 elfutils（autotools）
test_elfutils() {
	pkg=elfutils
	export CLANG_WRAP_DEBUG=1
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	export CC=clang-22 CXX=clang++-22 AR=llvm-ar-22
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		mkdir -p build-with-clangwrap && cd build-with-clangwrap
		rm -rf ~/tmp/llvmir/`pwd`
		../configure --disable-debuginfod --disable-libdebuginfod && \
			make V=1 -j`nproc` && \
			make install DESTDIR=`pwd`/install
		run_llvmir_cmds install/usr/local/lib/llvmir
		run_llvmir_cmds install/usr/local/lib/llvmir-bin
	)
}

# 测试 skalibs（skarnet 自定义构建系统）
test_skalibs() {
	pkg=skalibs
	export CLANG_WRAP_DEBUG=1
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	export CC=clang-22 CXX=clang++-22 AR=llvm-ar-22
	mkdir -p testdeb/$pkg
	(
		cd testdeb/$pkg
		rm -rf ${pkg}*
		apt-get source $pkg
		cd ${pkg}-*
		make clean >/dev/null 2>&1 || true
		rm -rf ~/tmp/llvmir/`pwd`
		./configure --prefix=/usr/local && \
			make V=1 -j`nproc` && \
			make install DESTDIR=`pwd`/install
		llvmir_build="$HOME/tmp/llvmir/`pwd`"
		if [ ! -f "$llvmir_build/libskarnet.so.xyzzy_cmd" ] || \
		   [ ! -f "$llvmir_build/libskarnet.a.xyzzy" ]; then
			echo "ERROR: skalibs 未生成 libskarnet 的 LLVM IR" >&2
			return 1
		fi
		(cd "$llvmir_build" && bash libskarnet.so.xyzzy_cmd)
		llvm-ar-22 t "$llvmir_build/libskarnet.a.xyzzy" >/dev/null
	)
}

# 显示帮助信息
show_help() {
	echo "用法: $0 [命令]"
	echo ""
	echo "命令:"
	echo "  build        - 构建和安装 clang-wrap"
	echo "  libxml2      - 测试 libxml2 包"
	echo "  pcre2        - 测试 pcre2 包"
	echo "  flex         - 测试 flex 包"
	echo "  bamf         - 测试 bamf 包"
	echo "  hesiod       - 测试 hesiod 包"
	echo "  hwloc        - 测试 hwloc 包"
	echo "  identity4c   - 测试 identity4c 包"
	echo "  paps         - 测试 paps 包"
	echo "  x264         - 测试 x264 包"
	echo "  ffmpeg       - 测试 ffmpeg 包"
	echo "  sqlite3      - 测试 sqlite3 包"
	echo "  libpcap      - 测试 libpcap 包"
	echo "  gsl          - 测试 gsl 包"
	echo "  libfyaml     - 测试 libfyaml 包"
	echo "  pixman       - 测试 pixman 包（Meson）"
	echo "  xfsprogs     - 测试 xfsprogs 包"
	echo "  elfutils     - 测试 elfutils 包"
	echo "  skalibs      - 测试 skalibs 包"
	echo "  all          - 构建并测试所有包"
	echo "  help         - 显示此帮助信息"
	echo ""
	echo "无参数时默认执行 build"
}

# 主入口
main() {
	case "${1:-build}" in
		build)
			build
			;;
		libxml2)
			test_libxml2
			;;
		pcre2)
			test_pcre2
			;;
		flex)
			test_flex
			;;
		bamf)
			test_bamf
			;;
		hesiod)
			test_hesiod
			;;
		hwloc)
			test_hwloc
			;;
		identity4c)
			test_identity4c
			;;
		paps)
			test_paps
			;;
		x264)
			test_x264
			;;
		ffmpeg)
			test_ffmpeg
			;;
		sqlite3)
			test_sqlite3
			;;
		libpcap)
			test_libpcap
			;;
		gsl)
			test_gsl
			;;
		libfyaml)
			test_libfyaml
			;;
		pixman)
			test_pixman
			;;
		xfsprogs)
			test_xfsprogs
			;;
		elfutils)
			test_elfutils
			;;
		skalibs)
			test_skalibs
			;;
		all)
			build
			test_libxml2
			test_pcre2
			test_flex
			test_bamf
			test_hesiod
			test_hwloc
			test_identity4c
			test_paps
			test_x264
			test_ffmpeg
			test_sqlite3
			test_libpcap
			test_gsl
			test_libfyaml
			test_pixman
			test_xfsprogs
			test_elfutils
			test_skalibs
			;;
		help|-h|--help)
			show_help
			;;
		*)
			echo "错误: 未知命令 '$1'"
			show_help
			exit 1
			;;
	esac
}

main "$@"
