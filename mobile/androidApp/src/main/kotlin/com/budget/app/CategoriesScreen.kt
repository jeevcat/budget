package com.budget.app

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.budget.shared.api.BudgetMode
import com.budget.shared.viewmodel.CategoriesViewModel
import com.budget.shared.viewmodel.CategoryTreeItem

// -- Kanagawa palette colors for budget modes --------------------------------

private val CrystalBlue = Color(0xFF7E9CD8)
private val OniViolet = Color(0xFF957FB8)
private val AutumnYellow = Color(0xFFDCA561)

private fun modeColor(mode: BudgetMode?): Color =
    when (mode) {
      BudgetMode.MONTHLY -> CrystalBlue
      BudgetMode.ANNUAL -> OniViolet
      BudgetMode.PROJECT -> AutumnYellow
      null -> Color.Unspecified
    }

private fun modeLabel(mode: BudgetMode?): String? =
    when (mode) {
      BudgetMode.MONTHLY -> "Monthly"
      BudgetMode.ANNUAL -> "Annual"
      BudgetMode.PROJECT -> "Project"
      null -> null
    }

// -- Root screen -------------------------------------------------------------

@Composable
fun CategoriesScreen(
    viewModel: CategoriesViewModel,
    modifier: Modifier = Modifier,
    onAddCategory: () -> Unit = {},
    onEditCategory: (String) -> Unit = {},
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  Box(modifier = modifier.fillMaxSize()) {
    when {
      state.loading && state.treeItems.isEmpty() -> {
        CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
      }
      state.error != null && state.treeItems.isEmpty() -> {
        Column(
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
          Text(
              text = state.error ?: "Unknown error",
              color = MaterialTheme.colorScheme.error,
          )
          Spacer(modifier = Modifier.height(12.dp))
          TextButton(onClick = viewModel::refresh) { Text("Retry") }
        }
      }
      state.treeItems.isEmpty() -> {
        Text(
            text = "No categories found.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.align(Alignment.Center).padding(24.dp),
        )
      }
      else -> {
        val visible = CategoriesViewModel.visibleItems(state.treeItems, state.expandedParents)
        CategoriesContent(
            visibleItems = visible,
            expandedParents = state.expandedParents,
            onToggleParent = viewModel::toggleParent,
            onCategoryClick = onEditCategory,
        )
      }
    }
    FloatingActionButton(
        onClick = onAddCategory,
        modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
    ) {
      Icon(Icons.Default.Add, contentDescription = "Add category")
    }
  }
}

// -- Content -----------------------------------------------------------------

@Composable
private fun CategoriesContent(
    visibleItems: List<CategoryTreeItem>,
    expandedParents: Set<String>,
    onToggleParent: (String) -> Unit,
    onCategoryClick: (String) -> Unit,
) {
  val groups = buildList {
    var current: MutableList<CategoryTreeItem>? = null
    for (item in visibleItems) {
      if (item.depth == 0) {
        current = mutableListOf(item)
        add(current)
      } else {
        current?.add(item)
      }
    }
  }

  LazyColumn(
      modifier = Modifier.fillMaxSize(),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 80.dp),
      verticalArrangement = Arrangement.spacedBy(12.dp),
  ) {
    items(groups, key = { it.first().id }) { group ->
      val root = group.first()
      CategoryGroupCard(
          root = root,
          children = group.drop(1),
          expanded = root.id in expandedParents,
          onToggle = { onToggleParent(root.id) },
          onCategoryClick = onCategoryClick,
      )
    }
  }
}

// -- Group card per root category --------------------------------------------

@Composable
private fun CategoryGroupCard(
    root: CategoryTreeItem,
    children: List<CategoryTreeItem>,
    expanded: Boolean,
    onToggle: () -> Unit,
    onCategoryClick: (String) -> Unit,
) {
  ElevatedCard(modifier = Modifier.fillMaxWidth().animateContentSize()) {
    Column {
      // Root header row
      Row(
          modifier = Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(16.dp),
          verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(
            text = root.name,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.weight(1f),
        )
        BudgetModeIndicator(mode = root.budgetMode)
        Spacer(modifier = Modifier.width(8.dp))
        Text(
            text = formatBudgetAmount(root.budgetAmount),
            style = MaterialTheme.typography.bodyMedium,
            color =
                if (root.budgetAmount != null) MaterialTheme.colorScheme.onSurface
                else MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (root.hasChildren) {
          Spacer(modifier = Modifier.width(4.dp))
          Icon(
              imageVector = if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
              contentDescription = if (expanded) "Collapse" else "Expand",
              tint = MaterialTheme.colorScheme.onSurfaceVariant,
          )
        }
      }

      // Child rows
      if (expanded) {
        for (child in children) {
          CategoryTreeRow(item = child, onClick = { onCategoryClick(child.id) })
        }
        Spacer(modifier = Modifier.height(8.dp))
      }
    }
  }
}

// -- Individual tree row (depth >= 1) ----------------------------------------

@Composable
private fun CategoryTreeRow(item: CategoryTreeItem, onClick: () -> Unit) {
  val startPadding =
      when (item.depth) {
        1 -> 40.dp
        else -> 64.dp
      }

  Row(
      modifier =
          Modifier.fillMaxWidth()
              .clickable(onClick = onClick)
              .padding(start = startPadding, end = 16.dp, top = 6.dp, bottom = 6.dp),
      verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
        text = item.name,
        style = MaterialTheme.typography.bodyMedium,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.weight(1f),
    )
    BudgetModeIndicator(mode = item.budgetMode)
    Spacer(modifier = Modifier.width(8.dp))
    Text(
        text = formatBudgetAmount(item.budgetAmount),
        style = MaterialTheme.typography.bodyMedium,
        color =
            if (item.budgetAmount != null) MaterialTheme.colorScheme.onSurface
            else MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}

// -- Budget mode dot + label -------------------------------------------------

@Composable
private fun BudgetModeIndicator(mode: BudgetMode?) {
  val label = modeLabel(mode) ?: return
  val color = modeColor(mode)
  Row(verticalAlignment = Alignment.CenterVertically) {
    Box(modifier = Modifier.size(8.dp).clip(CircleShape).background(color))
    Spacer(modifier = Modifier.width(4.dp))
    Text(
        text = label,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
  }
}
