# OSKernel2025-MonkeyOS
<div align="center">
  <img src="./logo.jpg" alt="logo" width="200" style="display:inline-block;"/>
  <img src="./tju.jpg" alt="tju" width="200" style="display:inline-block; margin-left: 20px;"/>
</div>



## 如何构建本项目

0. 在windows某目录下：
git clone https://gitlab.eduxiji.net/T202510056995244/monkeyos.git
1. 下载比赛用两个镜像解压至目录（sdcard-rv、sdcard-la）
Releases · oscomp/testsuits-for-oskernel
2. 下载这两个文件至目录（不用解压）
从 https://gitlab.educg.net/wangmingjian/os-contest-2024-image
下载gcc-13.2.0-loongarch64-linux-gnu.tgz 和qemu-9.2.1.tar.xz -
3. 会出现这样的问题（可能）
root@f048c1b57f1f:/mnt# make all
Building RISC-V kernel...
error: the listed checksum of `/mnt/vendor/bitflags/src/tests/from_bits_truncate.rs` has changed:
expected: d3406b5e107ebb6449b98a59eee6cc5d84f947d4aaee1ee7e80dc7202de179f0
actual:   6f0b901e6c02fe7ae549ea91192560ea76fdf5a1d3b9b4d0f42c2efeaa9ccf61
directory sources are not intended to be edited, if modifications are required then it is recommended that `[patch]` is used with a forked copy of the source
make: *** [Makefile:26: build-riscv] Error 1
解决方案，把gitlab里的vendor重新下到项目里
4. 编译本项目：make all
5. 运行本项目
Riscv：
qemu-system-riscv64 -machine virt -kernel kernel-rv -m 1G -nographic -smp 1 -bios default -drive file=sdcard-rv.img,if=none,format=raw,id=x0 -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 -no-reboot -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc
Loongarch：
qemu-system-loongarch64 -kernel kernel-la -m 1G -nographic -smp 1 -drive file=sdcard-la.img,if=none,format=raw,id=x0 -device virtio-blk-pci,drive=x0 -no-reboot -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc
