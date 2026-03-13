{
  description = "Budget – personal finance tracker";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    bun2nix.url = "github:nix-community/bun2nix";
    bun2nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      bun2nix,
    }:
    let
      nixosModule =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.budget;

          configFile = pkgs.writeText "budget-config.toml" ''
            database_url = "${cfg.databaseUrl}"
            llm_model = "${cfg.llmModel}"
            budget_currency = "${cfg.budgetCurrency}"
            expected_salary_count = ${toString cfg.expectedSalaryCount}
            server_port = ${toString cfg.port}
            host = "${cfg.host}"
            frontend_dir = "${cfg.package}/share/budget/frontend"
            gemini_api_key = "${cfg.geminiApiKey}"
            secret_key = "${cfg.secretKey}"
            enable_banking_app_id = "${cfg.enableBanking.appId}"
            enable_banking_private_key_path = "${cfg.enableBanking.privateKeyFile}"
            ${lib.optionalString (cfg.logPath != null) ''log_path = "${cfg.logPath}"''}
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
              default = "gemini-3.1-flash-lite-preview";
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

            logPath = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "Path to a log file. When null, logs go to stderr only (journald).";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services.budget = {
              description = "Budget personal finance tracker";
              after = [
                "network.target"
              ]
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
    (flake-utils.lib.eachSystem
      [
        "x86_64-linux"
        "aarch64-linux"
      ]
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            config = {
              # Android SDK components pulled via androidenv are unfree
              allowUnfree = true;
              android_sdk.accept_license = true;
            };
          };
          craneLib = crane.mkLib pkgs;
          lib = pkgs.lib;
          bun2nixPkg = bun2nix.packages.${system}.default;

          # Deps source: only Cargo files, so deps cache isn't invalidated by
          # migration or frontend changes
          depsSrc = craneLib.cleanCargoSource ./.;

          # Full source: includes migrations (for sqlx::migrate! macro)
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./migrations
            ];
          };

          frontendDist = pkgs.stdenv.mkDerivation {
            pname = "budget-frontend";
            version = "0.1.0";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./frontend
                ./package.json
                ./bun.lock
              ];
            };
            nativeBuildInputs = [ bun2nixPkg.hook ];
            bunDeps = bun2nixPkg.fetchBunDeps {
              bunNix = ./bun.nix;
            };
            doCheck = false;
            buildPhase = ''
              bun build frontend/index.html --outdir frontend/dist --minify
            '';
            installPhase = ''
              cp -r frontend/dist $out
            '';
          };

          commonArgs = {
            strictDeps = true;
            env.SQLX_OFFLINE = "true"; # Nix sandbox has no DB; CI/sandbox use live PG instead
            nativeBuildInputs = with pkgs; [
              pkg-config
            ];
            buildInputs = with pkgs; [
              openssl
              postgresql
            ];
          };

          # Build external deps only (no workspace crates that use sqlx::migrate!)
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { src = depsSrc; });

          budget = craneLib.buildPackage (
            commonArgs
            // {
              inherit src cargoArtifacts;
              # Tests require a live PostgreSQL instance (sqlx::test)
              doCheck = false;
              env.BUDGET_GIT_REV = self.shortRev or self.dirtyShortRev or "unknown";

              postInstall = ''
                mkdir -p $out/share/budget
                cp -r --no-preserve=mode ${frontendDist} $out/share/budget/frontend
              '';
            }
          );
        in
        {
          packages.default = budget;

          checks.default = budget;

          devShells.default =
            let
              androidSdk =
                (pkgs.androidenv.composeAndroidPackages {
                  platformVersions = [ "36" ];
                  buildToolsVersions = [ "36.0.0" ];
                  includeEmulator = false;
                  includeNDK = false;
                }).androidsdk;
            in
            craneLib.devShell {
              inputsFrom = [ budget ];
              packages = [
                pkgs.biome
                pkgs.bun
                pkgs.jdk21_headless
                androidSdk
              ];
              env.DATABASE_URL = "postgresql://budget@localhost:5432/budget";
              env.JAVA_HOME = "${pkgs.jdk21_headless}";
              env.ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
              # Generic Linux aapt2 binaries can't run on NixOS — override with the nix-patched one
              env.GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/36.0.0/aapt2";
            };
        }
      )
    )
    // {
      nixosModules.default = nixosModule;
    };
}
