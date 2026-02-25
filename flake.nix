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

          configFile = pkgs.writeText "budget-config.toml" ''
            database_url = "${cfg.databaseUrl}"
            llm_model = "${cfg.llmModel}"
            bank_provider = "enable_banking"
            budget_currency = "${cfg.budgetCurrency}"
            expected_salary_count = ${toString cfg.expectedSalaryCount}
            server_port = ${toString cfg.port}
            host = "${cfg.host}"
            frontend_dir = "${cfg.package}/share/budget/frontend"
            gemini_api_key = "${cfg.geminiApiKey}"
            secret_key = "${cfg.secretKey}"
            enable_banking_app_id = "${cfg.enableBanking.appId}"
            enable_banking_private_key_path = "${cfg.enableBanking.privateKeyFile}"
          '';

          startScript = pkgs.writeShellScript "budget-start" ''
            set -euo pipefail
            CONFIG_DIR="''${STATE_DIRECTORY:-${cfg.dataDir}}/config/budget"
            mkdir -p "$CONFIG_DIR"
            cp ${configFile} "$CONFIG_DIR/default-config.toml"
            chmod 600 "$CONFIG_DIR/default-config.toml"
            export XDG_CONFIG_HOME="''${STATE_DIRECTORY:-${cfg.dataDir}}/config"
            exec ${cfg.package}/bin/budget
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

            databaseUrl = lib.mkOption {
              type = lib.types.str;
              description = "PostgreSQL connection URL.";
            };

            dataDir = lib.mkOption {
              type = lib.types.path;
              default = "/var/lib/budget";
              description = "Directory for runtime state.";
            };

            host = lib.mkOption {
              type = lib.types.str;
              description = "Public URL for OAuth callbacks.";
            };

            secretKey = lib.mkOption {
              type = lib.types.str;
              description = "Bearer token secret for API authentication.";
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

            geminiApiKey = lib.mkOption {
              type = lib.types.str;
              description = "Gemini API key.";
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
              after = [ "network.target" ]
                ++ lib.optional (lib.hasPrefix "postgres" cfg.databaseUrl) "postgresql.service";
              wantedBy = [ "multi-user.target" ];

              serviceConfig = {
                Type = "simple";
                DynamicUser = true;
                StateDirectory = "budget";
                ExecStart = startScript;
                WorkingDirectory = cfg.dataDir;

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
            postgresql
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
