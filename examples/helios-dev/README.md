# Helios Dev Machine

This is a minimal falcon setup for creating a Helios development machine.

# If you've not installed

From the top dir in the falcon repo.

```
./get-propolis.sh
./get-ovmf.sh
./setup-base-images.sh
```

## tl;dr

** >>> Change `igb0` in src/main.rs to whatever your internet-facing host interface
is. <<< **

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

```
<ctl>-<q> # to escape from serial console session
```

then to destroy the vm

```shell
pfexec ../../target/debug/helios-dev destroy
```
