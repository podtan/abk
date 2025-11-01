# Contributing to trustee-config

Thank you for your interest in contributing to trustee-config! This document provides guidelines for contributing to the project.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/trustee-config.git`
3. Create a new branch: `git checkout -b feature/your-feature-name`
4. Make your changes
5. Run tests: `cargo test`
6. Commit your changes: `git commit -m "feat: Add your feature"`
7. Push to your fork: `git push origin feature/your-feature-name`
8. Open a Pull Request

## Development Guidelines

### Code Style

- Follow Rust standard formatting: `cargo fmt`
- Run clippy and fix warnings: `cargo clippy`
- Add documentation for public APIs
- Include examples in documentation where appropriate

### Testing

- Add tests for new functionality
- Ensure all tests pass: `cargo test`
- Maintain or improve code coverage
- Test edge cases and error conditions

### Commit Messages

Use conventional commit format:
- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation changes
- `test:` - Test additions or changes
- `refactor:` - Code refactoring
- `chore:` - Maintenance tasks

Example: `feat: Add support for YAML configuration files`

### Pull Requests

- Provide a clear description of the changes
- Reference any related issues
- Ensure CI passes
- Update CHANGELOG.md for notable changes
- Keep PRs focused on a single concern

## Code of Conduct

- Be respectful and inclusive
- Provide constructive feedback
- Help others learn and grow
- Focus on what's best for the project

## Questions?

Feel free to open an issue for:
- Bug reports
- Feature requests
- Questions about usage
- Discussion about the project

Thank you for contributing! 🎉
