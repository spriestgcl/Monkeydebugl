FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive


# 安装基础构建工具与常见依赖
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    curl \
    wget \
    sudo \
    git \
    ca-certificates \
    pkg-config \
    libglib2.0-dev \
    libfdt-dev \
    libpixman-1-dev \
    zlib1g-dev \
    ninja-build \
    python3 \
    python3-pip \
    python3-venv \
    gdb-multiarch \
    libssl-dev \
    libncurses-dev \
    libffi-dev \
    musl-tools \
    && pip install tomli \
    && apt-get clean

RUN wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250605/sdcard-la.img.xz -O /tmp/sdcard-la.img.xz && \
    wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250605/sdcard-rv.img.xz -O /tmp/sdcard-rv.img.xz && \
    apt-get update && apt-get install -y xz-utils && \
    unxz /tmp/sdcard-la.img.xz && \
    unxz /tmp/sdcard-rv.img.xz && \
    mv /tmp/sdcard-la.img /opt/sdcard-la.img && \
    mv /tmp/sdcard-rv.img /opt/sdcard-rv.img

RUN wget https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/2024.12.16/riscv64-elf-ubuntu-22.04-gcc-nightly-2024.12.16-nightly.tar.xz && \
    mkdir -p /opt/riscv && \
    tar -xJf riscv64-elf-ubuntu-22.04-gcc-nightly-2024.12.16-nightly.tar.xz -C /opt/riscv && \
    rm riscv64-elf-ubuntu-22.04-gcc-nightly-2024.12.16-nightly.tar.xz

ENV PATH="/opt/riscv/riscv64-elf-ubuntu-22.04-gcc-nightly-2024.12.16-nightly/bin:$PATH"
RUN apt-get update && apt-get install -y gcc-riscv64-unknown-elf
RUN apt-get update && apt-get install -y meson ninja-build git build-essential
RUN git clone https://gitlab.freedesktop.org/slirp/libslirp.git /tmp/libslirp && \
    cd /tmp/libslirp && \
    meson build && \
    ninja -C build && \
    ninja -C build install && \
    cd / && rm -rf /tmp/libslirp
# 安装 Rust 工具链（nightly）
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y && \
    . "$HOME/.cargo/env" && \
    rustup install nightly-2024-08-01 && \
    rustup default nightly-2024-08-01 && \
    rustup component add rust-src && \
    rustup component add llvm-tools-preview

ENV PATH="/root/.cargo/bin:$PATH"

# 安装 cargo-binutils

# 复制并解压 LoongArch 交叉编译器
COPY gcc-13.2.0-loongarch64-linux-gnu.tgz /tmp/
RUN mkdir -p /opt && \
    tar -C /opt -xzf /tmp/gcc-13.2.0-loongarch64-linux-gnu.tgz && \
    rm /tmp/gcc-13.2.0-loongarch64-linux-gnu.tgz

ENV PATH="/opt/gcc-13.2.0-loongarch64-linux-gnu/bin:$PATH"


ENV PATH="/opt/gcc-13.2.0-loongarch64-linux-gnu/bin:$PATH"

# 下载并解压 riscv64-musl 交叉编译器
RUN wget https://musl.cc/riscv64-linux-musl-cross.tgz && \
    tar -xvzf riscv64-linux-musl-cross.tgz && \
    rm riscv64-linux-musl-cross.tgz

ENV PATH="/riscv64-linux-musl-cross/bin:$PATH"

# 下载并编译 QEMU
# 复制并编译 QEMU
COPY qemu-9.2.1.tar.xz /tmp/
RUN tar -xf /tmp/qemu-9.2.1.tar.xz -C /tmp && \
    cd /tmp/qemu-9.2.1 && \
    ./configure --prefix=/qemu-bin-9.2.1 \
    --target-list=loongarch64-softmmu,riscv64-softmmu,aarch64-softmmu,x86_64-softmmu \
    --enable-slirp && \
    make -j$(nproc) && \
    make install && \
    cd / && rm -rf /tmp/qemu-9.2.1 /tmp/qemu-9.2.1.tar.xz


ENV PATH="/qemu-bin-9.2.1/bin:$PATH"

# 工作目录
WORKDIR /mnt/MONKEY_ByteOS

CMD ["/bin/bash"]