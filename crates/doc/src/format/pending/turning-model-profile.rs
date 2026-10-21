//! Turning OD jobs may store optional model_source (body name, reversed direction,
//! front origin, axis and approximation_mm) for a model-derived profile. Missing means manual input.
//! Source geometry is re-extracted before toolpath/NC generation and compared with
//! the saved contour and coordinate frame; stale profiles must be re-imported.
