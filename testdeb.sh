#!/bin/sh
set -e

# 代理设置
#export HTTPS_PROXY=10.200.7.1:8586

# 构建和安装 clang-wrap
build() {
	rm -rf clang-wrap-install
	cargo build --release
	cargo install --path . --root clang-wrap-install
	cd clang-wrap-install/bin
	ln -sf clang clang++
	ln -sf clang clang-22
	ln -sf clang clang++-22
	ln -sf ar llvm-ar
	ln -sf ar x86_64-linux-gnu-ar
	cd -
}

# 测试 libxml2
test_libxml2() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/libxml2
	cd testdeb/libxml2 && rm -rf libxml2*
	apt-get source libxml2
	cd libxml2-2*
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang-22 ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

# 测试 pcre2
test_pcre2() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=1
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/pcre2
	cd testdeb/pcre2 && rm -rf pcre2*
	apt-get source pcre2
	cd pcre2-*
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

# 测试 flex
test_flex() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/flex
	cd testdeb/flex && rm -rf flex*
	apt-get source flex
	cd flex-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

# 测试 bamf
test_bamf() {
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/bamf
	cd testdeb/bamf && rm -rf bamf*
	apt-get source bamf
	cd bamf-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	cd -
}

test_hesiod() {
	pkg=hesiod
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang-22 CXX=clang++-22 ../configure && \
		CC=clang-22 CXX=clang++-22 make V=1 && \
		make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_hwloc() {
	pkg=hwloc
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang CXX=clang++ ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_identity4c() {
	pkg=identity4c
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang CXX=clang++ ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	cd -
}

test_paps() {
	pkg=paps
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	dh_autoreconf
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang CXX=clang++ ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_x264() {
	pkg=x264
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang ../configure --enable-shared --disable-asm && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_ffmpeg() {
	pkg=ffmpeg
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	# ffmpeg 需要禁用汇编和一些可能有问题的组件
	CC=clang CXX=clang++ ../configure --cc=clang --cxx=clang++ --enable-shared --disable-asm --disable-doc --disable-htmlpages --disable-manpages --disable-podpages --disable-txtpages && make V=1 && make install V=1 DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_sqlite3() {
	pkg=sqlite3
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	mkdir -p build-with-clangwrap && cd build-with-clangwrap
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang CXX=clang++ ../configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	(cd install/usr/local/lib/llvmir-bin && bash *_cmd)
	cd -
}

test_libpcap() {
	pkg=libpcap
	export CLANG_WRAP_DEBUG=1
	#export EMIT_LLVMIR="-march=native"
	export EMIT_LLVMIR=-march=native
	export PATH=`pwd`/clang-wrap-install/bin:$PATH
	mkdir -p testdeb/$pkg
	cd testdeb/$pkg && rm -rf ${pkg}*
	apt-get source $pkg
	cd ${pkg}-*
	rm -rf ~/tmp/llvmir/`pwd`
	CC=clang CXX=clang++ ./configure && make V=1 && make install DESTDIR=`pwd`/install
	(cd install/usr/local/lib/llvmir && bash *_cmd)
	cd -
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
