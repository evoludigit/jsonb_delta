# Contributing to jsonb_delta

Thank you for your interest in contributing to jsonb_delta! This document provides guidelines and instructions for contributing.

## Code of Conduct

This project adheres to a Code of Conduct. By participating, you are expected to uphold this code. Please read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for details.

## How to Contribute

### Reporting Bugs

Before creating bug reports, please check existing issues to avoid duplicates. When creating a bug report, include:

- A clear, descriptive title
- Detailed steps to reproduce the issue
- Expected behavior vs actual behavior
- PostgreSQL version
- jsonb_delta version
- Any relevant error messages or logs

### Suggesting Enhancements

Enhancement suggestions are welcome! Please provide:

- A clear, descriptive title
- Detailed explanation of the proposed functionality
- Use cases and benefits
- Examples of how it would work

### Pull Requests

1. **Fork and Clone**
   ```bash
   git clone git@github.com:YOUR_USERNAME/jsonb_delta.git
   cd jsonb_delta
   ```

2. **Set Up Development Environment**
   ```bash
   # Install Rust toolchain
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

   # Install pgrx
   cargo install --locked cargo-pgrx --version 0.16.1
   cargo pgrx init

   # Install just (task runner)
   cargo install just
   ```

3. **Create a Branch**
   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b fix/your-bug-fix
   ```

4. **Make Your Changes**
   - Write clean, documented code
   - Follow existing code style
   - Add tests for new functionality
   - Update documentation as needed

5. **Test Your Changes**
   ```bash
   # Run all tests
   just test

   # Run specific test types
   just test-rust    # Rust unit tests
   just test-sql     # SQL integration tests

   # Check formatting and linting
   just check

   # Auto-fix issues
   just fix
   ```

6. **Commit Your Changes**
   ```bash
   git add .
   git commit -m "feat: add amazing feature"
   ```

   Use [Conventional Commits](https://www.conventionalcommits.org/):
   - `feat:` - New feature
   - `fix:` - Bug fix
   - `docs:` - Documentation changes
   - `test:` - Test additions/changes
   - `refactor:` - Code refactoring
   - `perf:` - Performance improvements
   - `chore:` - Maintenance tasks

7. **Push and Create PR**
   ```bash
   git push origin feature/your-feature-name
   ```

   Then create a Pull Request on GitHub with:
   - Clear description of changes
   - Link to related issues
   - Test results

## Development Guidelines

### Code Style

- Follow Rust conventions and idioms
- Use `rustfmt` for formatting (run `just fix`)
- Keep functions focused and well-named
- Add comments for complex logic
- Document public APIs with doc comments

### Testing

- Add tests for all new functionality
- Ensure existing tests pass
- Include both unit tests and integration tests
- Test edge cases and error conditions
- Aim for high code coverage

### PostgreSQL Compatibility

- Test against multiple PostgreSQL versions (13-18)
- Use pgrx features appropriately
- Document version-specific behavior

### Performance

- Consider performance implications
- Add benchmarks for performance-critical code
- Profile before optimizing
- Document performance characteristics

### Documentation

- Update README.md for user-facing changes
- Add inline code documentation
- Update TESTING.md for test-related changes
- Follow documentation standards in `docs/contributing/documentation-standards.md`

## Project Structure

```
jsonb_delta/
├── src/              # Rust source code
│   ├── lib.rs       # Main library entry point
│   └── ...
├── sql/              # SQL schema files
├── test/             # Test files
│   ├── sql/         # SQL integration tests
│   └── ...
├── docs/             # Documentation
├── .github/          # GitHub Actions workflows
└── justfile          # Task definitions
```

## Getting Help

- Check existing [documentation](README.md)
- Search [existing issues](https://github.com/evoludigit/jsonb_delta/issues)
- Ask questions by creating a new issue with the "question" label

## License

By contributing, you agree that your contributions will be licensed under the PostgreSQL License.

## Recognition

Contributors will be recognized in the project's documentation and release notes.

Thank you for contributing to jsonb_delta!
