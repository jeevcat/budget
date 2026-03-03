package com.budget.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetType
import com.budget.shared.viewmodel.CategoryEditViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CategoryEditScreen(
    viewModel: CategoryEditViewModel,
    onBack: () -> Unit,
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()

  LaunchedEffect(state.saved) { if (state.saved) onBack() }

  Scaffold(
      topBar = {
        TopAppBar(
            title = { Text(if (state.isEditing) "Edit Category" else "New Category") },
            navigationIcon = {
              IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
              }
            },
            actions = {
              TextButton(onClick = viewModel::save, enabled = !state.saving) { Text("Save") }
            },
            colors =
                TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                ),
        )
      },
  ) { innerPadding ->
    LazyColumn(
        modifier = Modifier.padding(innerPadding).fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
      if (state.saving) {
        item { LinearProgressIndicator(modifier = Modifier.fillMaxWidth()) }
      }

      val error = state.error
      if (error != null) {
        item {
          Text(
              text = error,
              color = MaterialTheme.colorScheme.error,
              style = MaterialTheme.typography.bodyMedium,
          )
        }
      }

      // Name field
      item {
        OutlinedTextField(
            value = state.name,
            onValueChange = viewModel::updateName,
            label = { Text("Name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
            enabled = !state.saving,
        )
      }

      // Parent category picker
      item { ParentPicker(viewModel = viewModel) }

      // Budget mode chips
      item { BudgetModeSelector(viewModel = viewModel) }

      // Budget type chips (shown when a budgetable mode is selected)
      if (state.budgetMode != null && state.budgetMode != BudgetMode.SALARY) {
        item { BudgetTypeSelector(viewModel = viewModel) }
      }

      // Budget amount (shown when a budgetable mode is selected)
      if (state.budgetMode != null && state.budgetMode != BudgetMode.SALARY) {
        item {
          OutlinedTextField(
              value = state.budgetAmount,
              onValueChange = viewModel::updateBudgetAmount,
              label = { Text("Budget amount") },
              singleLine = true,
              modifier = Modifier.fillMaxWidth(),
              enabled = !state.saving,
          )
        }
      }

      // Project date fields (shown when mode is PROJECT)
      if (state.budgetMode == BudgetMode.PROJECT) {
        item {
          OutlinedTextField(
              value = state.projectStartDate,
              onValueChange = viewModel::updateProjectStartDate,
              label = { Text("Start date (YYYY-MM-DD)") },
              singleLine = true,
              modifier = Modifier.fillMaxWidth(),
              enabled = !state.saving,
          )
        }
        item {
          OutlinedTextField(
              value = state.projectEndDate,
              onValueChange = viewModel::updateProjectEndDate,
              label = { Text("End date (YYYY-MM-DD)") },
              singleLine = true,
              modifier = Modifier.fillMaxWidth(),
              enabled = !state.saving,
          )
        }
      }
    }
  }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ParentPicker(viewModel: CategoryEditViewModel) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()
  var showSheet by remember { mutableStateOf(false) }

  val selectedName = state.availableParents.find { it.id == state.parentId }?.name

  ElevatedCard(modifier = Modifier.fillMaxWidth()) {
    ListItem(
        headlineContent = { Text("Parent category") },
        supportingContent = {
          Text(
              text = selectedName ?: "None",
              color =
                  if (selectedName != null) MaterialTheme.colorScheme.onSurface
                  else MaterialTheme.colorScheme.onSurfaceVariant,
          )
        },
        trailingContent =
            if (state.parentId != null) {
              {
                IconButton(onClick = { viewModel.updateParentId(null) }) {
                  Icon(Icons.Default.Close, contentDescription = "Clear parent")
                }
              }
            } else {
              null
            },
        modifier = Modifier.clickable(enabled = !state.saving) { showSheet = true },
    )
  }

  if (showSheet) {
    ParentPickerSheet(
        parents = state.availableParents.map { it.id to it.name },
        selectedId = state.parentId,
        onSelect = { id ->
          viewModel.updateParentId(id)
          showSheet = false
        },
        onDismiss = { showSheet = false },
    )
  }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ParentPickerSheet(
    parents: List<Pair<String, String>>,
    selectedId: String?,
    onSelect: (String) -> Unit,
    onDismiss: () -> Unit,
) {
  val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

  ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState) {
    Column(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
      Text(
          text = "Choose parent category",
          style = MaterialTheme.typography.titleMedium,
          fontWeight = FontWeight.Bold,
      )
      Spacer(modifier = Modifier.height(12.dp))

      if (parents.isEmpty()) {
        Text(
            text = "No available parent categories",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(vertical = 16.dp),
        )
      } else {
        LazyColumn(modifier = Modifier.fillMaxWidth().weight(1f, fill = false)) {
          items(parents, key = { it.first }) { (id, name) ->
            ListItem(
                headlineContent = { Text(name) },
                trailingContent =
                    if (id == selectedId) {
                      { Icon(Icons.Default.Check, contentDescription = "Selected") }
                    } else {
                      null
                    },
                modifier = Modifier.clickable { onSelect(id) },
            )
          }
        }
      }

      Spacer(modifier = Modifier.height(16.dp))
    }
  }
}

@Composable
private fun BudgetModeSelector(viewModel: CategoryEditViewModel) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()
  val modes =
      listOf(
          BudgetMode.MONTHLY to "Monthly",
          BudgetMode.ANNUAL to "Annual",
          BudgetMode.PROJECT to "Project",
          BudgetMode.SALARY to "Salary",
      )

  Column {
    Text(
        text = "Budget mode",
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(modifier = Modifier.height(8.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
      FilterChip(
          selected = state.budgetMode == null,
          onClick = { viewModel.updateBudgetMode(null) },
          label = { Text("None") },
          enabled = !state.saving,
      )
      for ((mode, label) in modes) {
        FilterChip(
            selected = state.budgetMode == mode,
            onClick = { viewModel.updateBudgetMode(if (state.budgetMode == mode) null else mode) },
            label = { Text(label) },
            enabled = !state.saving,
        )
      }
    }
  }
}

@Composable
private fun BudgetTypeSelector(viewModel: CategoryEditViewModel) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()
  val types = listOf(BudgetType.VARIABLE to "Variable", BudgetType.FIXED to "Fixed")

  Column {
    Text(
        text = "Budget type",
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    Spacer(modifier = Modifier.height(8.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
      for ((type, label) in types) {
        FilterChip(
            selected = state.budgetType == type,
            onClick = { viewModel.updateBudgetType(type) },
            label = { Text(label) },
            enabled = !state.saving,
        )
      }
    }
  }
}
