export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/26.2.11394342
export PATH=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH
export RUSTFLAGS="-Ctarget-feature=+fp16,+neon"
cargo apk build -r 

