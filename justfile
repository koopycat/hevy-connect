set shell := ["bash", "-cu"]

# Show available recipes
default:
    @just --list

# Install locked dependencies
install:
    pnpm install --frozen-lockfile

# Run the CLI from source
run *ARGS:
    pnpm exec tsx src/bin/hevy-axi.ts {{ARGS}}

# Build the distributable CLI
build:
    pnpm build

# Run unit and integration tests
test:
    pnpm test

# Run lint, formatting, types, tests, and build
check:
    pnpm check

# Link the CLI into the active pnpm environment
link:
    pnpm build
    pnpm add --global "{{justfile_directory()}}"
