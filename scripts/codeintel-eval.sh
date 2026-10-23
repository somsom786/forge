#!/usr/bin/env sh
# Reproduce the code-intelligence evaluation in docs/EVALUATION.md: clone the pinned public repositories (blobless, into
# $1 or ./.codeintel-eval), check out the pinned commits, and run the evaluation test. Nothing is executed from the clones;
# FORGE only reads them with git and tree-sitter.
set -eu
dir=${1:-.codeintel-eval}
mkdir -p "$dir"
repos="
ripgrep https://github.com/BurntSushi/ripgrep.git 6892bc2b7efe5909cbbeec8f757ed2c4c76b1d30
ky https://github.com/sindresorhus/ky.git 3541888878c4d303a8bc2ad61815f4e048f55a5d
click https://github.com/pallets/click.git 2247b35ea1c47c727d7a06e51fa280e12a863ff6
express https://github.com/expressjs/express.git 1c6f1b0fe4000977b9a7b4f34097e4d527dd9824
zod https://github.com/colinhacks/zod.git afdac65e0f80c0ccf9e8909da97c411567908b57
fd https://github.com/sharkdp/fd.git e3eead9da73fb6bcc41f692304e3c8f968a3ceb1
axios https://github.com/axios/axios.git aa54a5e1e0e50cd1a2561b854ae5490ff589d065
flask https://github.com/pallets/flask.git d086db856be187255b8ec61ef409357393020f32
hono https://github.com/honojs/hono.git 5ca49b0720755c585c9a9835f0a479ceea36fb52
"
list=""
echo "$repos" | while read -r name url sha; do
  [ -n "$name" ] || continue
  [ -d "$dir/$name" ] || git clone -q --filter=blob:none "$url" "$dir/$name"
  git -C "$dir/$name" -c advice.detachedHead=false checkout -q --detach "$sha"
done
for name in ripgrep ky click express zod fd axios flask hono; do list="$list${list:+,}$(cd "$dir/$name" && pwd)"; done
cd "$(dirname "$0")/../apps/desktop/src-tauri"
FORGE_CODEINTEL_EVAL="$list" FORGE_CODEINTEL_EVAL_OUT="${FORGE_CODEINTEL_EVAL_OUT:-codeintel-eval.json}" \
  cargo test --release --lib eval_oss -- --nocapture
