DB_URL := "postgres://postgres:postgres@localhost/moksha-mint"

# list all tasks
default:
  @just --list

# install all dependencies
deps:
  cargo install sqlx-cli typos-cli  grcov wasm-pack wasm-opt just


# clean cargo
clean:
  cargo clean


# check code for typos
[no-exit-message]
typos:
  #!/usr/bin/env bash
  >&2 echo '💡 Valid new words can be added to `typos.toml`'
  typos


# fix all typos
[no-exit-message]
typos-fix-all:
  #!/usr/bin/env bash
  >&2 echo '💡 Valid new words can be added to `typos.toml`'
  typos --write-changes


# format code, check typos and run tests
final-check:
  cargo fmt --all
  just typos
  cargo test
  just run-itests
  just build-wasm


#run coverage
run-coverage:
  #!/usr/bin/env bash
  mkdir -p target/coverage
  CARGO_INCREMENTAL=0 RUSTFLAGS='-Cinstrument-coverage' LLVM_PROFILE_FILE='cargo-test-%p-%m.profraw' cargo test
  grcov . --binary-path ./target/debug/deps/ -s . -t lcov --branch --ignore-not-existing --ignore '../*' --ignore "/*" -o target/coverage/tests.lcov
  grcov . --binary-path ./target/debug/deps/ -s . -t html --branch --ignore-not-existing --ignore '../*' --ignore "/*" -o target/coverage/html
  find . -name '*.profraw' -exec rm -r {} \;
  >&2 echo '💡 Created the report in target/coverage/html`'


# run the cashu-mint
run-mint *ARGS:
  RUST_BACKTRACE=1 MINT_APP_ENV=dev cargo run --bin moksha-mint -- {{ARGS}}

# run cli-wallet with the given args
run-cli *ARGS:
  RUST_BACKTRACE=1 cargo run --bin moksha-cli -- -m http://127.0.0.1:3338 -d ./data/wallet  {{ARGS}} 

# runs all tests
run-tests:
  RUST_BACKTRACE=1 cargo test --workspace --exclude integrationtests


# run integrationtests
run-itests:
    RUST_BACKTRACE=1 cargo test -p integrationtests

# build the mint docker-image
build-docker:
    docker build --build-arg COMMITHASH=$(git rev-parse HEAD) --build-arg BUILDTIME=$(date -u '+%F-%T') -t moksha-mint:latest .


# compile all rust crates, that are relevant for the client, to wasm
build-wasm:
   cargo +nightly build -p  moksha-core -p moksha-wallet \
   --target wasm32-unknown-unknown \
   -Z build-std=std,panic_abort


# runs sqlx prepare
db-prepare:
  cd moksha-mint && \
  cargo sqlx prepare --database-url {{ DB_URL }}

# runs sqlx prepare
db-migrate:
  cd moksha-mint && \
  cargo sqlx migrate run --database-url {{ DB_URL }}

# creates the postgres database
db-create:
  cd moksha-mint && \
  cargo sqlx database create --database-url {{ DB_URL }}


# publish everything on crates.io
publish:
  cargo publish -p moksha-core
  cargo publish -p moksha-wallet
  cargo publish -p moksha-mint
  cargo publish -p moksha-cli

   
