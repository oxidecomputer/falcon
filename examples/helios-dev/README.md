# Helios Dev Machine

This is a minimal falcon setup for creating a Helios development machine. It's a
one machine topology. The basic workflow with falcon is

1. Compile the topology program.
2. Launch the topology from the compiled topology program.
3. Get serial console access to nodes in the topology through the topology
   program.

# If you've not installed

From the top dir in the falcon repo.

```
./get-propolis.sh
./get-ovmf.sh
./setup-base-images.sh
```

## tl;dr

**>>> Change `igb0` in src/main.rs to whatever your internet-facing host interface
is. <<<**

```shell
cargo build
pfexec ../../target/debug/helios-dev launch
../../target/debug/helios-dev serial masaka
<enter> # to see console
exit
root
<enter> # blank root password
resize # match serial console to your terminal size
# do development things
```

when you're finished

```shell
<ctl>-<q> # to escape from serial console session
```

then to destroy the vm

```shell
pfexec ../../target/debug/helios-dev destroy
```
