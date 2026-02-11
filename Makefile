PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin

.PHONY: build install uninstall clean check-claude

check-claude:
	@command -v claude >/dev/null 2>&1 || { \
		echo "error: 'claude' CLI not found in PATH"; \
		echo ""; \
		echo "claudesh requires the Claude CLI to function."; \
		echo "Install it from: https://docs.anthropic.com/en/docs/claude-code"; \
		exit 1; \
	}

build: check-claude
	cargo build --release

install: build
	install -d $(DESTDIR)$(BINDIR)
	install -m 755 target/release/claudesh $(DESTDIR)$(BINDIR)/claudesh
	@echo ""
	@echo "installed claudesh to $(DESTDIR)$(BINDIR)/claudesh"
	@echo ""
	@echo "to use as your login shell:"
	@echo "  echo $(DESTDIR)$(BINDIR)/claudesh | sudo tee -a /etc/shells"
	@echo "  chsh -s $(DESTDIR)$(BINDIR)/claudesh"

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/claudesh
	@echo "removed claudesh from $(DESTDIR)$(BINDIR)"

clean:
	cargo clean
