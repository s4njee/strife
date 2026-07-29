import prettier from 'eslint-config-prettier'
import solid from 'eslint-plugin-solid'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: ['dist', 'node_modules'],
  },
  ...tseslint.configs.recommended,
  solid.configs['flat/typescript'],
  prettier,
)
