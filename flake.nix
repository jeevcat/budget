{
  description = "Budget – personal finance tracker";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
    }:
    let
      nixosModule =
        { config, lib, pkgs, ... }:
        let
          cfg = config.services.budget;
          configDir = "${cfg.dataDir}/config";

          # TOML config with non-secret values only
          baseConfig = pkgs.writeText "budget-base-config.toml" ''
            database_url = "sqlite:budget.db?mode=rwc"
            llm_model = "${cfg.llmModel}"
            bank_provider = "enable_banking"
            budget_currency = "${cfg.budgetCurrency}"
            expected_salary_count = ${toString cfg.expectedSalaryCount}
            server_port = ${toString cfg.port}
            host = "${cfg.host}"
            frontend_dir = "${cfg.package}/share/budget/frontend"
          '';

          # Script to merge secrets into config at runtime
          writeConfig = pkgs.writeShellScript "budget-write-config" ''
            set -euo pipefail
            mkdir -p "${configDir}/budget"

            SECRET_KEY="$(cat "$CREDENTIALS_DIRECTORY/secret-key")"
            EB_KEY_PATH="$CREDENTIALS_DIRECTORY/eb-private-key"
            GEMINI_KEY="$(cat "$CREDENTIALS_DIRECTORY/gemini-api-key")"
            EB_APP_ID="${cfg.enableBanking.appId}"

            cp ${baseConfig} "${configDir}/budget/default-config.toml"
            chmod 600 "${configDir}/budget/default-config.toml"

            cat >> "${configDir}/budget/default-config.toml" <<EOF
            secret_key = "$SECRET_KEY"
            enable_banking_app_id = "$EB_APP_ID"
            enable_banking_private_key_path = "$EB_KEY_PATH"
            gemini_api_key = "$GEMINI_KEY"
            EOF
          '';
        in
        {
          options.services.budget = {
            enable = lib.mkEnableOption "Budget personal finance tracker";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              description = "The budget package to use.";
            };

            port = lib.mkOption {
              type = lib.types.port;
              default = 3000;
              description = "HTTP listen port.";
            };

            dataDir = lib.mkOption {
              type = lib.types.path;
              default = "/var/lib/budget";
              description = "Directory for the SQLite database and runtime state.";
            };

            host = lib.mkOption {
              type = lib.types.str;
              description = "Public URL for OAuth callbacks.";
            };

            secretKeyFile = lib.mkOption {
              type = lib.types.path;
              description = "File containing the bearer token secret.";
            };

            enableBanking = {
              appId = lib.mkOption {
                type = lib.types.str;
                description = "Enable Banking application ID.";
              };

              privateKeyFile = lib.mkOption {
                type = lib.types.path;
                description = "Path to Enable Banking private key PEM file.";
              };
            };

            geminiApiKeyFile = lib.mkOption {
              type = lib.types.path;
              description = "File containing the Gemini API key.";
            };

            llmModel = lib.mkOption {
              type = lib.types.str;
              default = "gemini-2.5-flash-lite";
              description = "LLM model name for category suggestions.";
            };

            budgetCurrency = lib.mkOption {
              type = lib.types.str;
              default = "EUR";
              description = "Currency code for budget calculations.";
            };

            expectedSalaryCount = lib.mkOption {
              type = lib.types.int;
              default = 1;
              description = "Expected number of salary transactions per month.";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services.budget = {
              description = "Budget personal finance tracker";
              after = [ "network.target" ];
              wantedBy = [ "multi-user.target" ];

              serviceConfig = {
                Type = "simple";
                DynamicUser = true;
                StateDirectory = "budget";
                ExecStartPre = writeConfig;
                ExecStart = "${cfg.package}/bin/budget";
                WorkingDirectory = cfg.dataDir;
                ReadWritePaths = [ cfg.dataDir ];
                Environment = [ "XDG_CONFIG_HOME=${configDir}" ];

                LoadCredential = [
                  "secret-key:${cfg.secretKeyFile}"
                  "eb-private-key:${cfg.enableBanking.privateKeyFile}"
                  "gemini-api-key:${cfg.geminiApiKeyFile}"
                ];

                # Hardening
                NoNewPrivileges = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                PrivateTmp = true;
              };
            };
          };
        };
    in
    (flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
    ] (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        # Only include source files relevant to the Rust build
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./migrations
            ./frontend
          ];
        };
        lib = pkgs.lib;

        commonArgs = {
          inherit src;
          strictDeps = true;
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            openssl
            sqlite
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        budget = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;

            postInstall = ''
              mkdir -p $out/share/budget
              cp -r frontend $out/share/budget/frontend
            '';
          }
        );
      in
      {
        packages.default = budget;

        checks.default = budget;
      }
    ))
    // {
      nixosModules.default = nixosModule;
    };
}
