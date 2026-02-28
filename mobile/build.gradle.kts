plugins {
  alias(libs.plugins.androidApplication) apply false
  alias(libs.plugins.androidKmpLibrary) apply false
  alias(libs.plugins.kotlinMultiplatform) apply false
  alias(libs.plugins.composeCompiler) apply false
  alias(libs.plugins.kotlinSerialization) apply false
  alias(libs.plugins.spotless)
  alias(libs.plugins.detekt) apply false
}

spotless {
  kotlin {
    target("**/*.kt")
    targetExclude("**/build/**")
    ktfmt()
  }
  kotlinGradle {
    target("**/*.gradle.kts")
    targetExclude("**/build/**")
    ktfmt()
  }
}

subprojects {
  afterEvaluate {
    apply(plugin = rootProject.libs.plugins.detekt.get().pluginId)
    configure<dev.detekt.gradle.extensions.DetektExtension> {
      buildUponDefaultConfig = true
      allRules = true
      parallel = true
      config.setFrom(rootProject.file("detekt.yml"))
    }
  }
}
