# Makefile for clamguard

BINARY_NAME=clamguard
TARGET_DIR=target/release
INSTALL_DIR=$(HOME)/Library/Application\ Support/clamguard
PLIST_NAME=com.clamguard.plist
PLIST_PATH=$(HOME)/Library/LaunchAgents/$(PLIST_NAME)
LOG_DIR=$(HOME)/Library/Logs/clamguard

AGENT_LOG_DIR=$(HOME)/Library/Logs/clamguard
APP_NAME=ClamGuard.app
APP_CONTENTS=$(APP_NAME)/Contents

.PHONY: all build install uninstall clean logs audit install-agent uninstall-agent install-daemon uninstall-daemon scans logs-agent app

all: build

build:
	cargo build --release

install: install-agent

install-daemon: build
	@echo "Installing $(BINARY_NAME) to $(INSTALL_DIR) as a LaunchDaemon (root)..."
	sudo mkdir -p $(INSTALL_DIR)
	sudo cp $(TARGET_DIR)/$(BINARY_NAME) $(INSTALL_DIR)/$(BINARY_NAME)
	sudo chmod +x $(INSTALL_DIR)/$(BINARY_NAME)
	
	@echo "Creating log directory $(LOG_DIR)..."
	sudo mkdir -p $(LOG_DIR)
	sudo chmod 755 $(LOG_DIR)
	
	@echo "Installing LaunchDaemon..."
	sudo cp $(PLIST_NAME) $(PLIST_PATH)
	sudo chown root:wheel $(PLIST_PATH)
	sudo chmod 644 $(PLIST_PATH)
	
	@echo "Loading service..."
	sudo launchctl load -w $(PLIST_PATH)
	@echo "Installation complete. Service is running as root."

uninstall:
	@echo "Unloading and removing services if they exist..."
	-launchctl unload -w $(PLIST_PATH) 2>/dev/null
	-sudo launchctl unload -w /Library/LaunchDaemons/$(PLIST_NAME) 2>/dev/null
	
	@echo "Removing files from new and legacy locations..."
	-rm "$(INSTALL_DIR)/$(BINARY_NAME)"
	-rm $(PLIST_PATH)
	-sudo rm /usr/local/bin/$(BINARY_NAME)
	-sudo rm /Library/LaunchDaemons/$(PLIST_NAME)
	
	@echo "Uninstallation complete. Logs remain at their respective locations."

uninstall-daemon:
	@echo "Unloading legacy daemon..."
	-sudo launchctl unload -w /Library/LaunchDaemons/$(PLIST_NAME) 2>/dev/null
	
	@echo "Removing legacy files..."
	-sudo rm /usr/local/bin/$(BINARY_NAME)
	-sudo rm /Library/LaunchDaemons/$(PLIST_NAME)
	
	@echo "Legacy daemon uninstalled."

install-agent: build
	@echo "Installing $(BINARY_NAME) to $(INSTALL_DIR)..."
	mkdir -p "$(INSTALL_DIR)"
	cp $(TARGET_DIR)/$(BINARY_NAME) "$(INSTALL_DIR)/$(BINARY_NAME)"
	chmod +x "$(INSTALL_DIR)/$(BINARY_NAME)"

	@echo "Installing LaunchAgent..."
	mkdir -p $(HOME)/Library/LaunchAgents
	# Update plist paths during copy to use user-local paths
	sed 's|/usr/local/bin/clamguard|$(INSTALL_DIR)/$(BINARY_NAME)|g; s|/Library/Logs/clamguard|$(LOG_DIR)|g' $(PLIST_NAME) > $(PLIST_PATH)
	chmod 644 $(PLIST_PATH)

	@echo "Loading service..."
	-launchctl unload $(PLIST_PATH) 2>/dev/null
	launchctl load -w $(PLIST_PATH)
	@echo "Installation complete. Tray icon should appear in the menu bar."

uninstall-agent:
	@echo "Unloading agent..."
	-launchctl unload -w $(PLIST_PATH) 2>/dev/null

	@echo "Removing files..."
	-rm "$(INSTALL_DIR)/$(BINARY_NAME)"
	-rm $(PLIST_PATH)

	@echo "Agent uninstalled."

clean:
	cargo clean
	rm -rf $(APP_NAME)

app: build
	@echo "Creating application bundle $(APP_NAME)..."
	rm -rf $(APP_NAME)
	mkdir -p $(APP_CONTENTS)/MacOS
	mkdir -p $(APP_CONTENTS)/Resources
	cp $(TARGET_DIR)/$(BINARY_NAME) $(APP_CONTENTS)/MacOS/$(BINARY_NAME)
	cp Info.plist $(APP_CONTENTS)/Info.plist
	echo "APPL????" > $(APP_CONTENTS)/PkgInfo
	chmod +x $(APP_CONTENTS)/MacOS/$(BINARY_NAME)
	@echo "Application bundle created. You can now double-click $(APP_NAME) in Finder."

logs:
	@echo "Showing both system and user logs (if available)..."
	-tail -f $(LOG_DIR)/stdout.log $(LOG_DIR)/stderr.log $(AGENT_LOG_DIR)/audit.log $(AGENT_LOG_DIR)/clamav_external_scans.log /tmp/clamguard.agent.stdout.log /tmp/clamguard.agent.stderr.log 2>/dev/null

logs-agent:
	tail -f $(AGENT_LOG_DIR)/audit.log $(AGENT_LOG_DIR)/clamav_external_scans.log /tmp/clamguard.agent.stdout.log /tmp/clamguard.agent.stderr.log

audit:
	-tail -f $(LOG_DIR)/audit.log $(AGENT_LOG_DIR)/audit.log

scans:
	-tail -f $(LOG_DIR)/clamav_external_scans.log $(AGENT_LOG_DIR)/clamav_external_scans.log
