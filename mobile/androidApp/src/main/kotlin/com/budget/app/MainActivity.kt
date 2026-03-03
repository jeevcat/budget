package com.budget.app

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Category
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Receipt
import androidx.compose.material.icons.outlined.Category
import androidx.compose.material.icons.outlined.Dashboard
import androidx.compose.material.icons.outlined.Receipt
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavDestination.Companion.hasRoute
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.budget.shared.config.AndroidConfigStore
import com.budget.shared.config.ServerConfig
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DefaultBudgetRepository
import com.budget.shared.viewmodel.CategoriesViewModel
import com.budget.shared.viewmodel.CategoryEditViewModel
import com.budget.shared.viewmodel.DashboardViewModel
import com.budget.shared.viewmodel.TransactionsViewModel
import kotlinx.serialization.Serializable

@Serializable data object BudgetRoute

@Serializable data object TransactionsRoute

@Serializable data class TransactionDetailRoute(val transactionId: String)

@Serializable data object CategoriesRoute

@Serializable data class CategoryEditRoute(val categoryId: String? = null)

internal data class TopLevelRoute(
    val label: String,
    val route: Any,
    val selectedIcon: ImageVector,
    val unselectedIcon: ImageVector,
)

private val TOP_LEVEL_ROUTES =
    listOf(
        TopLevelRoute("Budget", BudgetRoute, Icons.Filled.Dashboard, Icons.Outlined.Dashboard),
        TopLevelRoute(
            "Transactions",
            TransactionsRoute,
            Icons.Filled.Receipt,
            Icons.Outlined.Receipt,
        ),
        TopLevelRoute(
            "Categories",
            CategoriesRoute,
            Icons.Filled.Category,
            Icons.Outlined.Category,
        ),
    )

class MainActivity : ComponentActivity() {

  private lateinit var configStore: AndroidConfigStore
  private var currentConfig by mutableStateOf<ServerConfig?>(null)

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    enableEdgeToEdge()

    configStore = AndroidConfigStore(applicationContext)
    currentConfig = configStore.load()

    setContent {
      val darkTheme = isSystemInDarkTheme()
      val colorScheme =
          when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
              val context = LocalContext.current
              if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            }
            darkTheme -> darkColorScheme()
            else -> lightColorScheme()
          }
      MaterialTheme(colorScheme = colorScheme) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background,
        ) {
          val config = currentConfig
          if (config != null) {
            AppShell(
                config = config,
                onLogout = {
                  configStore.clear()
                  currentConfig = null
                },
            )
          } else {
            SetupScreen(
                configStore = configStore,
                onConnected = { newConfig -> currentConfig = newConfig },
            )
          }
        }
      }
    }
  }
}

@Composable
internal fun AppShell(config: ServerConfig, onLogout: () -> Unit) {
  val navController = rememberNavController()

  val repository = remember(config) { DefaultBudgetRepository(config.serverUrl, config.apiKey) }

  val dashboardVm: DashboardViewModel = viewModel { DashboardViewModel(repository) }
  val transactionsVm: TransactionsViewModel = viewModel { TransactionsViewModel(repository) }
  val categoriesVm: CategoriesViewModel = viewModel { CategoriesViewModel(repository) }

  val transactionsState by transactionsVm.uiState.collectAsStateWithLifecycle()

  AppNavHost(
      navController = navController,
      repository = repository,
      dashboardVm = dashboardVm,
      transactionsVm = transactionsVm,
      categoriesVm = categoriesVm,
      uncategorizedCount = transactionsState.total,
      onLogout = onLogout,
  )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DetailScaffold(
    title: String,
    onBack: () -> Unit,
    content: @Composable (Modifier) -> Unit,
) {
  Scaffold(
      topBar = {
        TopAppBar(
            title = { Text(title) },
            navigationIcon = {
              IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
              }
            },
            colors =
                TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
        )
      },
  ) { innerPadding ->
    content(Modifier.padding(innerPadding))
  }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TopLevelScaffold(
    navController: NavHostController,
    title: String,
    uncategorizedCount: Int,
    onLogout: () -> Unit,
    content: @Composable (Modifier) -> Unit,
) {
  val navBackStackEntry by navController.currentBackStackEntryAsState()
  val currentDestination = navBackStackEntry?.destination

  Scaffold(
      topBar = {
        TopAppBar(
            title = { Text(title) },
            colors =
                TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
            actions = {
              IconButton(onClick = onLogout) {
                Icon(
                    Icons.AutoMirrored.Filled.Logout,
                    contentDescription = "Disconnect",
                )
              }
            },
        )
      },
      bottomBar = {
        NavigationBar {
          TOP_LEVEL_ROUTES.forEach { route ->
            val selected = currentDestination?.hasRoute(route.route::class) == true
            NavigationBarItem(
                selected = selected,
                onClick = {
                  navController.navigate(route.route) {
                    popUpTo(navController.graph.startDestinationId) { saveState = true }
                    launchSingleTop = true
                    restoreState = true
                  }
                },
                icon = {
                  val icon = if (selected) route.selectedIcon else route.unselectedIcon
                  if (route.route is TransactionsRoute && uncategorizedCount > 0) {
                    BadgedBox(badge = { Badge { Text("$uncategorizedCount") } }) {
                      Icon(icon, contentDescription = route.label)
                    }
                  } else {
                    Icon(icon, contentDescription = route.label)
                  }
                },
                label = { Text(route.label) },
            )
          }
        }
      },
  ) { innerPadding ->
    content(Modifier.padding(innerPadding))
  }
}

@Composable
private fun AppNavHost(
    navController: NavHostController,
    repository: BudgetRepository,
    dashboardVm: DashboardViewModel,
    transactionsVm: TransactionsViewModel,
    categoriesVm: CategoriesViewModel,
    uncategorizedCount: Int,
    onLogout: () -> Unit,
) {
  NavHost(
      navController = navController,
      startDestination = BudgetRoute,
  ) {
    composable<BudgetRoute> {
      val dashboardState by dashboardVm.uiState.collectAsStateWithLifecycle()
      val selectedCategoryId = dashboardState.selectedCategoryId

      if (selectedCategoryId != null) {
        val categoryName =
            dashboardState.monthlyStatuses
                .find { it.categoryId == selectedCategoryId }
                ?.categoryName
                ?: dashboardState.annualStatuses
                    .find { it.categoryId == selectedCategoryId }
                    ?.categoryName
                ?: dashboardState.projects
                    .find { it.categoryId == selectedCategoryId }
                    ?.categoryName
        DetailScaffold(
            title = categoryName ?: "Category",
            onBack = { dashboardVm.selectCategory(null) },
        ) { modifier ->
          DashboardContent(
              viewModel = dashboardVm,
              onTransactionClick = { id ->
                navController.navigate(TransactionDetailRoute(transactionId = id))
              },
              modifier = modifier,
          )
        }
      } else {
        TopLevelScaffold(navController, "Budget", uncategorizedCount, onLogout) { modifier ->
          DashboardContent(
              viewModel = dashboardVm,
              onTransactionClick = { id ->
                navController.navigate(TransactionDetailRoute(transactionId = id))
              },
              modifier = modifier,
          )
        }
      }
    }
    composable<TransactionsRoute> {
      TopLevelScaffold(navController, "Transactions", uncategorizedCount, onLogout) { modifier ->
        TransactionsScreen(
            viewModel = transactionsVm,
            onTransactionClick = { id ->
              navController.navigate(TransactionDetailRoute(transactionId = id))
            },
            modifier = modifier,
        )
      }
    }
    composable<TransactionDetailRoute> { backStackEntry ->
      val route = backStackEntry.toRoute<TransactionDetailRoute>()
      val state by transactionsVm.uiState.collectAsStateWithLifecycle()
      LaunchedEffect(route.transactionId) {
        transactionsVm.selectTransactionById(route.transactionId)
      }
      TransactionDetailScreen(
          state = state,
          viewModel = transactionsVm,
          onBack = { navController.popBackStack() },
      )
    }
    composable<CategoriesRoute> {
      TopLevelScaffold(navController, "Categories", uncategorizedCount, onLogout) { modifier ->
        CategoriesScreen(
            viewModel = categoriesVm,
            onAddCategory = { navController.navigate(CategoryEditRoute()) },
            onEditCategory = { id -> navController.navigate(CategoryEditRoute(categoryId = id)) },
            modifier = modifier,
        )
      }
    }
    composable<CategoryEditRoute> { backStackEntry ->
      val route = backStackEntry.toRoute<CategoryEditRoute>()
      val editingCategory =
          route.categoryId?.let { id -> categoriesVm.uiState.value.categories.find { it.id == id } }
      val editVm: CategoryEditViewModel =
          viewModel(key = route.categoryId ?: "new") {
            CategoryEditViewModel(repository, editingCategory)
          }
      CategoryEditScreen(
          viewModel = editVm,
          onBack = { navController.popBackStack() },
      )
    }
  }
}
