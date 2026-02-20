.PHONY: coverage coverage-html coverage-lcov

## Generate and display a coverage summary in the terminal
coverage:
	cargo llvm-cov --all-features

## Generate an HTML coverage report and open it
coverage-html:
	cargo llvm-cov --all-features --html --open

## Generate an LCOV report (for CI / Codecov upload)
coverage-lcov:
	cargo llvm-cov --all-features --lcov --output-path lcov.info
