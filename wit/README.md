# Generic WASM Provider Interface

This directory contains the **generic WIT (WebAssembly Interface Types)** definition that ALL WASM providers must implement to work with Simpaticoder.

## Architecture

Simpaticoder uses a **provider-agnostic** WASM architecture:

1. **No hardcoded provider names** - The WIT interface is generic (`simpaticoder:provider`)
2. **Dynamic discovery** - Providers are loaded from `providers/*/provider.wasm`
3. **Standard interface** - All providers implement the same WIT interface
4. **Zero recompilation** - Add new providers by dropping `.wasm` files

## WIT Interface (`provider.wit`)

The `provider.wit` file defines the standard interface that all WASM providers must export:

### Core Functions

- `get-provider-metadata()` - Returns JSON with provider name, version, models, features
- `build-headers()` - Builds custom HTTP headers for API requests
- `format-request()` - Formats request body for the provider's API
- `format-request-from-json()` - Formats complex requests with tool calls
- `parse-response()` - Parses API response into standard format
- `handle-stream-chunk()` - Processes SSE streaming chunks

### Type Definitions

All providers use these standard types:
- `message` - Chat message (role, content)
- `tool` - Tool/function definition
- `tool-call` - Tool invocation in response
- `assistant-message` - Parsed assistant response
- `content-delta` - Streaming content delta
- `header-pair` - HTTP header key-value
- `provider-error` - Error with message and optional code

## Creating a New Provider

To create a WASM provider for Simpaticoder:

1. **Implement the WIT interface:**
   ```rust
   // In your provider's wit/world.wit
   package your-provider:provider@1.0.0;
   
   world provider {
       import simpaticoder:provider/adapter;
       export simpaticoder:provider/adapter;
   }
   ```

2. **Build to WASM:**
   ```bash
   cargo component build --release
   ```

3. **Deploy:**
   ```bash
   cp target/wasm32-wasi/release/your_provider.wasm ~/.simpaticoder/providers/your-provider/provider.wasm
   ```

4. **Use:**
   ```bash
   LLM_PROVIDER=your-provider simpaticoder run "your task"
   ```

## Example: Tanbal Provider

The Tanbal provider is a multi-backend provider that routes to OpenAI, GitHub Copilot, or Anthropic based on the model prefix:

- `openai/gpt-4` → OpenAI API
- `anthropic/claude-sonnet-4` → Anthropic API  
- Auto-detects backend from model string
- Single WASM binary supports all backends

See the tanbal-provider repository for implementation details.

## Benefits

✅ **Zero Code Changes** - New providers don't require Rust changes  
✅ **Provider Agnostic** - Simpaticoder doesn't know about specific providers  
✅ **Hot Reload** - Drop in new `.wasm` files without recompiling  
✅ **Version Independence** - Providers and core can version separately  
✅ **Standard Interface** - All providers use the same contract  

## Technical Details

- **Runtime**: `wasmtime` component model
- **Language**: Providers can be written in any WASM-compatible language (Rust, Go, C++, etc.)
- **Async**: Full async/await support via wasmtime-async
- **WASI**: Providers can use WASI for environment variables, file I/O
- **Namespace**: All providers use `simpaticoder:provider` namespace (no hardcoded names)
